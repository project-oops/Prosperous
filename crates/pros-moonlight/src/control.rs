//! The ENet control channel that carries input, on port 47999.
//!
//! Adapted from Moonshine's `control/mod.rs` (BSD-2-Clause; see `THIRD-PARTY-LICENSES.md`). A
//! client sends input as AES-128-GCM encrypted control messages over ENet; each controller update
//! is decoded ([`crate::input`]) and forwarded to the target's input port (9806) as a `PPAD`
//! record, the same record Porthole's `feed` sends.
//!
//! An encrypted message is `type(0x0001) | length | sequence | 16-byte GCM tag | ciphertext`; the
//! nonce is the sequence, zeros, then `HC`; the decrypted message is `type | length | payload`,
//! with controller input as type `0x0206`. Input is recognised by that type alone, and
//! [`crate::input::decode`] declines anything too short to be a pad.

use std::net::UdpSocket;
use std::time::Duration;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};
use pros_link::feed::{Feed, PORT as INPUT_PORT};
use pros_link::pad::SLOTS;
use rusty_enet::{Event, Host, HostSettings};

use crate::error::{Error, Result};
use crate::input;

/// The encrypted-message type, at the front of every control packet.
const ENCRYPTED: u16 = 0x0001;
/// The decrypted-message type that carries controller input.
const INPUT_DATA: u16 = 0x0206;
/// Bytes before the ciphertext: type(2) + length(2) + sequence(4) + tag(16).
const CIPHER_OFFSET: usize = 24;
/// Where the sequence sits in the packet.
const SEQUENCE_AT: usize = 4;
/// Where the GCM tag sits.
const TAG_AT: usize = 8;

/// Run the control channel until `keep_going` returns false.
///
/// `rikey` is the key the client sent at launch; `target` is the address of the target whose 9806
/// the decoded pads are forwarded to.
///
/// # Errors
///
/// If the port cannot be bound or the ENet host cannot be created.
pub(crate) fn run(
    port: u16,
    rikey: [u8; 16],
    target: &str,
    keep_going: impl Fn() -> bool,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_nonblocking(true)?;
    let host = Host::new(
        socket,
        HostSettings {
            peer_limit: usize::from(SLOTS),
            channel_limit: 8,
            ..Default::default()
        },
    )
    .map_err(|error| Error::Io(std::io::Error::other(format!("enet host: {error:?}"))))?;

    let cipher = Aes128Gcm::new_from_slice(&rikey)
        .map_err(|_| Error::Signature("control key was not 16 bytes".into()))?;
    let mut feed = Feed::new();
    if let Err(error) = feed.open(target, INPUT_PORT) {
        tracing::warn!(%error, "control channel could not open the target input port");
    }
    let mut sequences = [0_u32; SLOTS as usize];

    pump(host, &cipher, &mut feed, &mut sequences, &keep_going);
    Ok(())
}

/// The service loop: drain every pending ENet event, then sleep a millisecond.
fn pump(
    mut host: Host<UdpSocket>,
    cipher: &Aes128Gcm,
    feed: &mut Feed,
    sequences: &mut [u32],
    keep_going: &impl Fn() -> bool,
) {
    while keep_going() {
        loop {
            match host.service() {
                Ok(Some(event)) => handle(&event, cipher, feed, sequences),
                Ok(None) => break,
                Err(error) => {
                    tracing::debug!(?error, "enet service error");
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Act on one ENet event: a controller update on channel 0 becomes a `PPAD` record.
fn handle(
    event: &Event<'_, UdpSocket>,
    cipher: &Aes128Gcm,
    feed: &mut Feed,
    sequences: &mut [u32],
) {
    let Event::Receive {
        channel_id: 0,
        packet,
        ..
    } = event
    else {
        return;
    };
    let Some(update) = decrypt_input(cipher, packet.data()) else {
        return;
    };
    let slot = usize::from(update.slot).min(sequences.len().saturating_sub(1));
    let mut pad = update.pad;
    pad.sequence = sequences[slot];
    sequences[slot] = sequences[slot].wrapping_add(1);
    feed.send(&[pad.to_wire()]);
}

/// Decrypt one control packet and, if it is controller input, decode it.
fn decrypt_input(cipher: &Aes128Gcm, data: &[u8]) -> Option<input::Update> {
    if data.len() <= CIPHER_OFFSET || u16::from_le_bytes([data[0], data[1]]) != ENCRYPTED {
        return None;
    }
    // Nonce: the 4-byte sequence, six zero bytes, then "HC".
    let mut nonce = [0_u8; 12];
    nonce[0..4].copy_from_slice(&data[SEQUENCE_AT..SEQUENCE_AT + 4]);
    nonce[10] = b'H';
    nonce[11] = b'C';

    let tag = Tag::from_slice(&data[TAG_AT..TAG_AT + 16]);
    let mut plaintext = data[CIPHER_OFFSET..].to_vec();
    cipher
        .decrypt_in_place_detached(Nonce::from_slice(&nonce), &[], &mut plaintext, tag)
        .ok()?;

    // The decrypted message: type, length, then the input event at byte 8 for input.
    if plaintext.len() < 8 || u16::from_le_bytes([plaintext[0], plaintext[1]]) != INPUT_DATA {
        return None;
    }
    input::decode(&plaintext[8..])
}

#[cfg(test)]
mod tests {
    use super::{CIPHER_OFFSET, ENCRYPTED, INPUT_DATA, decrypt_input};
    use aes_gcm::aead::{AeadInPlace, KeyInit};
    use aes_gcm::{Aes128Gcm, Nonce};
    use pros_link::pad::Button;

    /// Encrypt a control message as a client does, to test decryption without an ENet peer.
    fn encrypt(key: &[u8; 16], sequence: u32, plaintext: &[u8]) -> Vec<u8> {
        let cipher = Aes128Gcm::new_from_slice(key).unwrap();
        let mut nonce = [0_u8; 12];
        nonce[0..4].copy_from_slice(&sequence.to_le_bytes());
        nonce[10] = b'H';
        nonce[11] = b'C';
        let mut buffer = plaintext.to_vec();
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &[], &mut buffer)
            .unwrap();

        let mut packet = Vec::new();
        packet.extend_from_slice(&ENCRYPTED.to_le_bytes());
        packet.extend_from_slice(&u16::try_from(buffer.len() + 20).unwrap().to_le_bytes());
        packet.extend_from_slice(&sequence.to_le_bytes());
        packet.extend_from_slice(&tag);
        packet.extend_from_slice(&buffer);
        assert_eq!(packet.len(), CIPHER_OFFSET + plaintext.len());
        packet
    }

    /// A decrypted input message: type 0x0206, a big-endian length at offset 4, then a controller
    /// packet at byte 8 (the layout `decrypt_input` reads).
    fn input_message(buttons: u32) -> Vec<u8> {
        // A 26-byte multi-controller packet with the buttons set.
        let mut pad = vec![0_u8; 26];
        pad[8..10].copy_from_slice(&u16::try_from(buttons & 0xffff).unwrap().to_le_bytes());

        let mut message = vec![0_u8; 8];
        message[0..2].copy_from_slice(&INPUT_DATA.to_le_bytes());
        message[4..8].copy_from_slice(&u32::try_from(pad.len()).unwrap().to_be_bytes());
        message.extend_from_slice(&pad);
        message
    }

    /// An encrypted input message decrypts and decodes to the pad it carries.
    #[test]
    fn an_encrypted_controller_message_decodes_to_a_pad() {
        let key = [3_u8; 16];
        let cipher = Aes128Gcm::new_from_slice(&key).unwrap();
        let packet = encrypt(&key, 1, &input_message(0x1000)); // Moonlight A -> Cross
        let update = decrypt_input(&cipher, &packet).expect("a controller update");
        assert!(update.pad.holds(Button::Cross));
    }

    /// Unencrypted packets and non-input messages produce no pad.
    #[test]
    fn a_packet_that_is_not_encrypted_or_not_input_is_ignored() {
        let key = [3_u8; 16];
        let cipher = Aes128Gcm::new_from_slice(&key).unwrap();
        assert!(
            decrypt_input(&cipher, &[0, 0, 0, 0]).is_none(),
            "wrong type"
        );
        // Encrypted, but a ping (0x0200), not input.
        let mut ping = input_message(0);
        ping[0..2].copy_from_slice(&0x0200_u16.to_le_bytes());
        let packet = encrypt(&key, 2, &ping);
        assert!(decrypt_input(&cipher, &packet).is_none(), "not input");
    }

    /// A packet under the wrong key fails the GCM tag and produces no pad.
    #[test]
    fn a_wrong_key_fails_the_tag_and_is_ignored() {
        let packet = encrypt(&[3_u8; 16], 1, &input_message(0x1000));
        let wrong = Aes128Gcm::new_from_slice(&[9_u8; 16]).unwrap();
        assert!(
            decrypt_input(&wrong, &packet).is_none(),
            "GCM tag must reject a wrong key"
        );
    }
}
