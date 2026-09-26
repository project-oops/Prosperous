//! Browsing the target's filesystem, and moving files across.
//!
//! The server accepts any credentials, so this logs in with the conventional anonymous pair.
//!
//! A session refuses to open unless the server agrees to binary mode, because text mode
//! rewrites line endings and a payload altered that way arrives intact-looking and fails to
//! run. The address in a passive reply is ignored and only its port used, because a server
//! behind address translation reports an address that may not be reachable. See
//! [`crate::files::port_from_passive`].

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::wire;

/// Service name a port override uses to move the file service off its usual port.
const SERVICE: &str = "ftpsrv";

/// Port the file service listens on.
const PORT: u16 = 2121;

/// How long to wait for either connection.
const CONNECT: Duration = Duration::from_secs(6);

/// How long a transfer may be silent before it is called dead.
///
/// An inactivity window, not a total: a large file legitimately takes minutes.
const QUIET: Duration = Duration::from_secs(30);

/// What kind of thing a listing line describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link. Whether it is followed is up to the server.
    Link,
    /// The line did not have the shape this crate knows how to read.
    ///
    /// Kept rather than dropped, so a listing never looks emptier than it is. The entry
    /// carries the line verbatim, and [`Entry::is_usable`] says it is not a path.
    Unrecognised,
}

/// One line of a directory listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The file name, when the line was understood. Otherwise the whole line.
    pub name: String,
    /// What it is.
    pub kind: Kind,
    /// Size in bytes, when the line carried one.
    pub size: Option<u64>,
    /// Exactly what the server sent, minus the line ending.
    ///
    /// The listing format is the server's choice; this is the evidence when a parse looks
    /// wrong.
    pub raw: String,
}

impl Entry {
    /// Whether [`Entry::name`] may be used as a path.
    ///
    /// False for a line that was not understood, whose name field holds the whole line.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        !matches!(self.kind, Kind::Unrecognised)
    }
}

/// One logged-in connection, reused across operations.
///
/// Browsing is a login followed by many listings, so the login is paid once. The free
/// functions below are one-shot forms for callers doing exactly one thing.
#[derive(Debug)]
pub struct Session {
    control: BufReader<TcpStream>,
    address: String,
}

impl Session {
    /// Opens and logs in.
    ///
    /// # Errors
    ///
    /// [`Error::Refused`] when the service is not loaded. [`Error::Rejected`] if the server
    /// declines the login or declines binary mode.
    pub fn open(link: &crate::Link) -> Result<Self> {
        Self::open_at(&link.address, link.port(SERVICE, PORT))
    }

    /// Opens against a port other than the usual one.
    ///
    /// Public for the same reason as [`crate::loader::send_at`].
    ///
    /// # Errors
    ///
    /// As [`Session::open`].
    pub fn open_at(address: &str, port: u16) -> Result<Self> {
        let stream = wire::connect(address, port, CONNECT)?;
        stream.set_read_timeout(Some(QUIET))?;
        stream.set_write_timeout(Some(QUIET))?;
        let mut session = Self {
            control: BufReader::new(stream),
            address: address.to_owned(),
        };

        session.expect("connecting", &[220])?;
        let hello = session.command("USER anonymous", &[230, 331])?;
        if hello.code == 331 {
            session.command("PASS anonymous", &[230, 202])?;
        }
        // Required: text mode edits the bytes in transit. See the module note.
        session.command("TYPE I", &[200])?;
        Ok(session)
    }

    /// Lists a directory.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the path does not exist.
    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let mut data = self.open_data(&format!("LIST {path}"))?;
        let mut bytes = Vec::new();
        data.read_to_end(&mut bytes)?;
        drop(data);
        self.expect("listing", &[226, 250])?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(text
            .lines()
            .filter(|line| !is_header(line))
            .map(parse_entry)
            .filter(|entry| !is_itself_or_its_parent(&entry.name))
            .collect())
    }

    /// Fetches a file whole.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the file is not there. A transfer that stops early is an
    /// [`Error::Io`], never a short result.
    pub fn retrieve(&mut self, path: &str) -> Result<Vec<u8>> {
        let mut data = self.open_data(&format!("RETR {path}"))?;
        let mut bytes = Vec::new();
        data.read_to_end(&mut bytes)?;
        drop(data);
        self.expect("retrieving", &[226, 250])?;
        Ok(bytes)
    }

    /// Writes a file, replacing anything already there.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the server will not take it - a read-only mount, or a
    /// directory that does not exist.
    pub fn store(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut data = self.open_data(&format!("STOR {path}"))?;
        data.write_all(bytes)?;
        data.flush()?;
        // The server learns the end of the file only from the socket closing.
        data.shutdown(Shutdown::Write)?;
        drop(data);
        self.expect("storing", &[226, 250])?;
        Ok(())
    }

    /// The size of a file on the target, in bytes.
    ///
    /// A successful `STOR` does not prove the bytes landed: a mounted file or an overlay can
    /// keep the old file and still reply with success. Comparing the size is the cheap check.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the file is not there or the server does not implement
    /// `SIZE`. [`Error::Unintelligible`] when it answers `213` without a number.
    pub fn size(&mut self, path: &str) -> Result<u64> {
        let reply = self.command(&format!("SIZE {path}"), &[213])?;
        // The number is the last token, so padding or an extra word before it is tolerated.
        reply
            .text
            .split_whitespace()
            .next_back()
            .and_then(|token| token.parse::<u64>().ok())
            .ok_or(Error::Unintelligible {
                doing: "reading a size".to_owned(),
                said: reply.text,
            })
    }

    /// Makes a directory, and is content if it is already there.
    ///
    /// Restoring a tree asks for every directory on the way down, most of which already
    /// exist, so an existing directory is success.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] for any other refusal: a read-only mount, or a missing parent.
    pub fn make_directory(&mut self, path: &str) -> Result<()> {
        self.send(&format!("MKD {path}"))?;
        let reply = self.reply("making a directory")?;
        // 2xx is completion (standard 257, or 226/250/200 from embedded servers); 521 and
        // 550 are how servers say it already exists.
        if succeeded(reply.code) || matches!(reply.code, 521 | 550) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: "making a directory".to_owned(),
            reply: reply.text,
        })
    }

    /// Removes a file.
    ///
    /// Files and directories are separate calls, so removing a directory is always explicit.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] with the server's own words when it is not there, is a directory,
    /// or the mount is read-only.
    pub fn delete_file(&mut self, path: &str) -> Result<()> {
        self.send(&format!("DELE {path}"))?;
        let reply = self.reply("deleting a file")?;
        if succeeded(reply.code) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: format!("deleting {path}"),
            reply: reply.text,
        })
    }

    /// Removes a directory, which every server refuses unless it is empty.
    ///
    /// Nothing here empties it first. The recursive walk lives in `pros_core::remove`, where
    /// its guards are tested against the fake.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`], including for the ordinary case of it not being empty.
    pub fn remove_directory(&mut self, path: &str) -> Result<()> {
        self.send(&format!("RMD {path}"))?;
        let reply = self.reply("removing a directory")?;
        if succeeded(reply.code) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: format!("removing {path}"),
            reply: reply.text,
        })
    }

    /// Says goodbye.
    ///
    /// Frees the slot in a small server's connection table. The reply is not awaited.
    pub fn close(mut self) {
        let _ = self.send("QUIT");
    }

    /// Opens a data connection and starts a transfer on it.
    fn open_data(&mut self, command: &str) -> Result<TcpStream> {
        let port = self.passive()?;
        // Connect before the command, or a fast server finishes with nowhere to send.
        let data = wire::connect(&self.address, port, CONNECT)?;
        data.set_read_timeout(Some(QUIET))?;
        data.set_write_timeout(Some(QUIET))?;
        self.command(command, &[125, 150])?;
        Ok(data)
    }

    /// Asks for a data port.
    fn passive(&mut self) -> Result<u16> {
        let reply = self.command("PASV", &[227])?;
        port_from_passive(&reply.text).ok_or(Error::Unintelligible {
            doing: "reading a passive reply".to_owned(),
            said: reply.text,
        })
    }

    /// Sends a command and reads what it produced.
    fn command(&mut self, command: &str, accepted: &[u16]) -> Result<Reply> {
        self.send(command)?;
        self.expect(first_word(command), accepted)
    }

    /// Writes one command line.
    fn send(&mut self, command: &str) -> Result<()> {
        let stream = self.control.get_mut();
        stream.write_all(command.as_bytes())?;
        stream.write_all(b"\r\n")?;
        stream.flush()?;
        Ok(())
    }

    /// Reads a reply and insists it is one of the ones that mean yes.
    fn expect(&mut self, doing: &str, accepted: &[u16]) -> Result<Reply> {
        let reply = self.reply(doing)?;
        if accepted.contains(&reply.code) {
            return Ok(reply);
        }
        Err(Error::Rejected {
            doing: doing.to_owned(),
            reply: reply.text,
        })
    }

    /// Reads one reply, however many lines it takes.
    fn reply(&mut self, doing: &str) -> Result<Reply> {
        let first = self.line(doing)?;
        let code = code_of(&first).ok_or(Error::Unintelligible {
            doing: doing.to_owned(),
            said: first.clone(),
        })?;

        let mut text = first.clone();
        // A hyphen in the fourth column means more lines follow, until one repeats the code
        // with a space. Unread lines would become the answer to the next command.
        if first.as_bytes().get(3) == Some(&b'-') {
            loop {
                let next = self.line(doing)?;
                let ended = code_of(&next) == Some(code) && next.as_bytes().get(3) != Some(&b'-');
                text.push('\n');
                text.push_str(&next);
                if ended {
                    break;
                }
            }
        }
        Ok(Reply { code, text })
    }

    /// Reads one line, treating a closed connection as an answer rather than an end.
    fn line(&mut self, doing: &str) -> Result<String> {
        let mut line = String::new();
        let read = self.control.read_line(&mut line)?;
        if read == 0 {
            return Err(Error::Unintelligible {
                doing: doing.to_owned(),
                said: "the connection closed part-way through a reply".to_owned(),
            });
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }
}

/// One reply from the server.
#[derive(Debug)]
struct Reply {
    /// The numeric code, which is what a decision is made on.
    code: u16,
    /// Everything the server said, including any continuation lines.
    text: String,
}

/// Whether a reply code means the command worked.
///
/// The whole 2xx family, which the protocol defines as completion: the target's ftpsrv
/// answers `DELE` with `226 File deleted` rather than the standard `250`.
const fn succeeded(code: u16) -> bool {
    code >= 200 && code < 300
}

/// Lists a directory over a connection opened for the purpose.
///
/// # Errors
///
/// As [`Session::open`] and [`Session::list`].
pub fn list(link: &crate::Link, path: &str) -> Result<Vec<Entry>> {
    let mut session = Session::open(link)?;
    let entries = session.list(path);
    session.close();
    entries
}

/// Fetches one file over a connection opened for the purpose.
///
/// # Errors
///
/// As [`Session::open`] and [`Session::retrieve`].
pub fn retrieve(link: &crate::Link, path: &str) -> Result<Vec<u8>> {
    let mut session = Session::open(link)?;
    let bytes = session.retrieve(path);
    session.close();
    bytes
}

/// Writes one file over a connection opened for the purpose.
///
/// # Errors
///
/// As [`Session::open`] and [`Session::store`].
pub fn store(link: &crate::Link, path: &str, bytes: &[u8]) -> Result<()> {
    let mut session = Session::open(link)?;
    let stored = session.store(path, bytes);
    session.close();
    stored
}

/// The port from a passive-mode reply, ignoring the address in it.
///
/// The reply carries four address numbers and two port numbers. The address is what the
/// server believes it has, which behind translation is not the one that reached it, so the
/// host already connected to is used instead. The six numbers are found by scanning, since
/// not every server parenthesises them.
#[must_use]
pub fn port_from_passive(reply: &str) -> Option<u16> {
    reply
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .find_map(six_numbers)
        .map(|numbers| u16::from(numbers[4]) * 256 + u16::from(numbers[5]))
}

/// Six comma-separated numbers that each fit in a byte, or nothing.
fn six_numbers(chunk: &str) -> Option<[u8; 6]> {
    let mut numbers = [0_u8; 6];
    let mut seen = 0;
    for field in chunk.split(',') {
        let value = field.parse::<u8>().ok()?;
        *numbers.get_mut(seen)? = value;
        seen += 1;
    }
    (seen == 6).then_some(numbers)
}

/// The three-digit code at the front of a reply line.
fn code_of(line: &str) -> Option<u16> {
    let head = line.get(..3)?;
    if !head.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    head.parse().ok()
}

/// The first word of a command, for saying what was being done when it failed.
fn first_word(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or(command)
}

/// Whether a line is the listing's own header rather than an entry.
///
/// `total <n>` begins every long-form listing; reported as unreadable it would appear as a
/// warning in every backup.
fn is_header(line: &str) -> bool {
    line.strip_prefix("total ")
        .is_some_and(|rest| !rest.is_empty() && rest.trim().chars().all(|c| c.is_ascii_digit()))
}

/// Whether this listing entry is the directory itself, or the one above it.
///
/// Filtered here rather than by each caller, because a recursive walk that follows `.` or
/// `..` copies without end or escapes the directory it was asked for.
fn is_itself_or_its_parent(name: &str) -> bool {
    name == "." || name == ".."
}

/// Reads one long-form listing line.
///
/// A line that does not fit becomes a [`Kind::Unrecognised`] entry carrying the line.
fn parse_entry(line: &str) -> Entry {
    let raw = line.trim_end_matches(['\r', '\n']).to_owned();
    let unrecognised = || Entry {
        name: raw.clone(),
        kind: Kind::Unrecognised,
        size: None,
        raw: raw.clone(),
    };

    let Some((columns, name)) = split_columns(&raw, 8) else {
        return unrecognised();
    };
    if name.is_empty() {
        return unrecognised();
    }
    let kind = match columns.first().and_then(|mode| mode.chars().next()) {
        Some('d') => Kind::Directory,
        Some('l') => Kind::Link,
        Some('-') => Kind::File,
        _ => return unrecognised(),
    };
    Entry {
        name: name.to_owned(),
        kind,
        size: columns.get(4).and_then(|field| field.parse().ok()),
        raw,
    }
}

/// Splits off `count` whitespace-separated columns, returning them and the rest.
///
/// Not `split_whitespace`: the tail is a file name that may contain spaces.
fn split_columns(line: &str, count: usize) -> Option<(Vec<&str>, &str)> {
    let mut rest = line;
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace)?;
        columns.push(rest.get(..end)?);
        rest = rest.get(end..)?;
    }
    Some((columns, rest.trim_start()))
}

#[cfg(test)]
mod tests {
    use super::{Kind, parse_entry, port_from_passive, succeeded};

    /// Every 2xx code the target uses for completion, `226 File deleted` included, is success.
    #[test]
    fn a_completion_code_this_target_uses_is_read_as_success() {
        for code in [200, 226, 250] {
            assert!(succeeded(code), "{code} means it worked");
        }
    }

    /// A code outside the 2xx family is not success, whatever its text says.
    #[test]
    fn a_refusal_is_still_a_refusal() {
        for code in [110, 150, 331, 425, 500, 550, 553] {
            assert!(!succeeded(code), "{code} is not a completion");
        }
    }

    /// The port is the last two numbers, combined as high and low bytes.
    #[test]
    fn a_passive_reply_gives_up_its_port() {
        assert_eq!(
            port_from_passive("227 Entering Passive Mode (192,168,1,50,195,80)"),
            Some(195 * 256 + 80)
        );
    }

    /// A passive reply without brackets still parses.
    #[test]
    fn a_passive_reply_without_brackets_still_parses() {
        assert_eq!(
            port_from_passive("227 entering passive mode 10,0,0,1,4,1"),
            Some(4 * 256 + 1)
        );
    }

    /// A reply without six byte-sized numbers yields no port.
    #[test]
    fn a_reply_with_no_six_numbers_in_it_gives_nothing() {
        assert_eq!(port_from_passive("500 Unknown command"), None);
        assert_eq!(port_from_passive("227 Passive mode (1,2,3)"), None);
    }

    /// A file name may contain spaces, so the tail of the line is not tokenised.
    #[test]
    fn a_name_with_spaces_survives_the_parse() {
        let entry = parse_entry("-rw-r--r--   1 root root  1048576 Aug 25 12:00 my report.txt");
        assert_eq!(entry.name, "my report.txt");
        assert_eq!(entry.kind, Kind::File);
        assert_eq!(entry.size, Some(1_048_576));
    }

    /// A directory is recognised by the first character of the mode field.
    #[test]
    fn a_directory_is_recognised_as_one() {
        let entry = parse_entry("drwxr-xr-x   2 root root        0 Aug 25 12:00 pldmgr");
        assert_eq!(entry.kind, Kind::Directory);
        assert_eq!(entry.name, "pldmgr");
    }

    /// The listing's `total` header is recognised, and a file named like it is not.
    #[test]
    fn the_listing_header_is_not_an_entry() {
        assert!(super::is_header("total 48"));
        assert!(super::is_header("total 0"));
        assert!(
            !super::is_header("total"),
            "a word on its own is not a header"
        );
        assert!(
            !super::is_header("totally-a-file.bin"),
            "a file whose name starts that way is a file"
        );
    }

    /// A line that is not understood is kept, verbatim, and marked unusable as a path.
    #[test]
    fn an_unreadable_line_is_kept_and_marked_unusable() {
        let entry = parse_entry("total 48");
        assert_eq!(entry.kind, Kind::Unrecognised);
        assert!(
            !entry.is_usable(),
            "an unread line must not be used as a path"
        );
        assert_eq!(entry.raw, "total 48");
    }
}
