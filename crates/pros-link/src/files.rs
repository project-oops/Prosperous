//! Browsing the target's filesystem, and moving files across.
//!
//! # Anonymous, and that is the design rather than an oversight
//!
//! The server takes any credentials and checks none of them. Every service in the chain is
//! unauthenticated on the local network by design - that is what a payload chain is - so
//! this logs in with a conventional anonymous pair and does not pretend the exchange means
//! anything.
//!
//! # Two guards, both against silence
//!
//! **Binary mode is checked, not assumed.** A transfer in the default text mode rewrites
//! line endings, and a payload that has had four bytes changed in the middle still arrives,
//! still reports a byte count, and still fails to run - with nothing anywhere saying why.
//! So the session refuses to open at all if the server will not agree to binary.
//!
//! **The address in a passive reply is thrown away.** A small server behind any kind of
//! translation reports the address it believes it has, which is regularly not the one that
//! reached it. The host already in hand is reachable by proof - a connection is open on it -
//! so only the port is taken from the reply. See [`crate::files::port_from_passive`].

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::wire;

/// Port the file service listens on.
/// Which service an override names when it moves this off its usual port.
const SERVICE: &str = "ftpsrv";

const PORT: u16 = 2121;

/// How long to wait for either connection.
const CONNECT: Duration = Duration::from_secs(6);

/// How long a transfer may be silent before it is called dead.
///
/// This is an **inactivity** window, not a total. A large file over a link measured at
/// tens of megabytes a second is minutes of legitimate transfer, and a total budget would
/// cut it off for being big.
const QUIET: Duration = Duration::from_secs(30);

/// What kind of thing a listing line describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link. Followed or not is the server's business, not this crate's.
    Link,
    /// The line did not have the shape this crate knows how to read.
    ///
    /// **Kept rather than dropped.** A listing that silently omits what it could not parse
    /// tells a person a directory is empty when it is not, which is the worst answer
    /// available. An entry marked this way carries the server's line verbatim so it can be
    /// shown, and [`Entry::is_usable`] says it must not be used as a path.
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
    /// Carried because a listing format is a server's choice and this one was written for
    /// a target rather than for a standard. When a parse looks wrong, this is the evidence
    /// of what it was parsing.
    pub raw: String,
}

impl Entry {
    /// Whether [`Entry::name`] may be used as a path.
    ///
    /// False for a line that was not understood, where the name field holds the whole line
    /// and using it would produce a request for a file that was never there.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        !matches!(self.kind, Kind::Unrecognised)
    }
}

/// One logged-in connection, reused across operations.
///
/// # Why a session rather than a function per operation
///
/// Every other service here is one exchange, so a free function is the whole interface.
/// This one is not: browsing is a login followed by a listing followed by another listing,
/// and a function that logged in each time would pay four round trips per directory a
/// person clicks on. The one-shot forms below exist for callers doing exactly one thing.
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
    /// declines the login or **declines binary mode**, which fails the whole session rather
    /// than continuing in a mode that corrupts payloads quietly.
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
        // Not a formality. See the module note: text mode edits the bytes in transit and
        // nothing downstream can tell that it happened.
        session.command("TYPE I", &[200])?;
        Ok(session)
    }

    /// Lists a directory.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the path does not exist, which is an ordinary answer and
    /// distinguishable from the connection failing.
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
    /// [`Error::Io`] rather than a short result - a truncated payload that reports success
    /// is the failure this crate exists to prevent.
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
        // The server is waiting for an end it can only learn from the socket closing.
        // Dropping alone would do it; saying so is clearer about why.
        data.shutdown(Shutdown::Write)?;
        drop(data);
        self.expect("storing", &[226, 250])?;
        Ok(())
    }

    /// Makes a directory, and is content if it is already there.
    ///
    /// **Already existing is not a failure.** Restoring a folder tree means asking for every
    /// directory on the way down, most of which will exist by the time the second file is
    /// written, and a caller that had to tell those two apart would have to parse replies -
    /// which is this crate's job, not theirs.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] for a refusal that is not *it is already there*: a read-only
    /// mount, or a parent that does not exist.
    pub fn make_directory(&mut self, path: &str) -> Result<()> {
        self.send(&format!("MKD {path}"))?;
        let reply = self.reply("making a directory")?;
        // 257 is made; 521 and 550 are the two ways servers say it is already there. A
        // directory that exists is the state the caller wanted either way.
        if matches!(reply.code, 257 | 521 | 550) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: "making a directory".to_owned(),
            reply: reply.text,
        })
    }

    /// Removes a file.
    ///
    /// # Why this is not called `remove` and does not take a directory
    ///
    /// A directory is a different command and a different risk. Deleting a file loses one
    /// thing somebody named; deleting a directory loses whatever is inside it, which they may
    /// not have looked at. **A caller that could pass either would sometimes pass the wrong
    /// one**, so the two are separate calls and the one that is dangerous is the one that has
    /// to be typed out.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`] when the server will not - it is not there, it is a directory, or
    /// the mount is read-only. The server's own words come back, because *no such file* and
    /// *permission denied* need different work from different people.
    pub fn delete_file(&mut self, path: &str) -> Result<()> {
        self.send(&format!("DELE {path}"))?;
        let reply = self.reply("deleting a file")?;
        if matches!(reply.code, 250 | 200) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: format!("deleting {path}"),
            reply: reply.text,
        })
    }

    /// Removes a directory, which every server refuses unless it is empty.
    ///
    /// **Nothing here empties one first.** A recursive delete over this protocol is a walk
    /// that issues a command per entry, and a walk that has gone wrong deletes things nobody
    /// listed - which is the same shape as the backup that climbed out of its own directory,
    /// with the consequences pointing the other way.
    ///
    /// So the server's refusal is passed on as it stands. Somebody who means to remove a full
    /// directory can empty it a file at a time, seeing each one.
    ///
    /// # Errors
    ///
    /// [`Error::Rejected`], including for the ordinary case of it not being empty.
    pub fn remove_directory(&mut self, path: &str) -> Result<()> {
        self.send(&format!("RMD {path}"))?;
        let reply = self.reply("removing a directory")?;
        if matches!(reply.code, 250 | 200) {
            return Ok(());
        }
        Err(Error::Rejected {
            doing: format!("removing {path}"),
            reply: reply.text,
        })
    }

    /// Says goodbye.
    ///
    /// A server with a small connection table notices the difference between this and
    /// walking away. The reply is not waited for: there is nothing a caller could do about
    /// a server that will not acknowledge a farewell.
    pub fn close(mut self) {
        let _ = self.send("QUIT");
    }

    /// Opens a data connection and starts a transfer on it.
    fn open_data(&mut self, command: &str) -> Result<TcpStream> {
        let port = self.passive()?;
        // Connect first, then ask. The other order lets a fast server finish before there
        // is anywhere for the answer to go.
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
        // A hyphen in the fourth column means more lines follow, until one repeats the
        // code with a space. Reading only the first line of one of these leaves the rest
        // in the buffer, where it becomes the answer to the *next* command.
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
/// # Why the address is discarded
///
/// The reply carries six numbers: four of address and two of port. A server reports the
/// address it believes it has, which behind any translation is not the one that reached
/// it - and a client that dials it connects to a machine on the wrong network, or to
/// nothing. The host already in hand arrived at this server by proof.
///
/// # Why the numbers are found rather than pattern-matched
///
/// The conventional reply parenthesises them and not every server does. Scanning for a run
/// of six that fit costs nothing and does not care about the punctuation around it.
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
/// **Not a guess.** `total 8` is part of the long-form listing format, and every listing
/// begins with one. Leaving it in as an unreadable line makes every directory look partly
/// unread - and a caller that reports what it could not copy would name it in every backup
/// it ever made, which is how a real warning gets ignored.
fn is_header(line: &str) -> bool {
    line.strip_prefix("total ")
        .is_some_and(|rest| !rest.is_empty() && rest.trim().chars().all(|c| c.is_ascii_digit()))
}

/// Reads one listing line.
///
/// The format is the long-form listing every server of this lineage produces. A line that
/// does not fit becomes a [`Kind::Unrecognised`] entry carrying the line itself rather
/// than disappearing - see [`Kind::Unrecognised`] for why that matters.
/// Whether this listing entry is the directory itself, or the one above it.
///
/// # Why this is filtered here and not by each caller
///
/// `.` and `..` are artefacts of how a listing is written, not things in the directory.
/// **Every caller wants them gone, so a caller that forgets is the only possible outcome** -
/// and one did: the browser filtered them and the recursive copy did not, so asking to back
/// up a 64KB folder walked into `.` twelve times and climbed out through `..` into the rest
/// of the filesystem.
///
/// That failure is quiet in the worst way. It does not error; it copies, steadily, with a
/// progress line that looks exactly like a large folder taking a while. The paths give it
/// away only if somebody reads them: `/data/homebrew/pkg/./././././././../../mini-syscore.elf`.
///
/// Filtering at the source means the next caller cannot make the same mistake.
fn is_itself_or_its_parent(name: &str) -> bool {
    name == "." || name == ".."
}

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
/// Not `split_whitespace`: the last field is a file name, which may contain spaces, so the
/// tail has to be kept whole rather than tokenised with the rest.
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
    use super::{Kind, parse_entry, port_from_passive};

    /// The two numbers that matter are the last two, and they combine as a pair of bytes.
    #[test]
    fn a_passive_reply_gives_up_its_port() {
        assert_eq!(
            port_from_passive("227 Entering Passive Mode (192,168,1,50,195,80)"),
            Some(195 * 256 + 80)
        );
    }

    /// The punctuation is a convention, not a rule, and a server is allowed to differ.
    #[test]
    fn a_passive_reply_without_brackets_still_parses() {
        assert_eq!(
            port_from_passive("227 entering passive mode 10,0,0,1,4,1"),
            Some(4 * 256 + 1)
        );
    }

    /// Something that is not a passive reply produces nothing, rather than a plausible
    /// port number assembled out of whatever digits were lying around.
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

    /// A directory is told from a file by the first character, which is the only part of
    /// the mode field this needs.
    #[test]
    fn a_directory_is_recognised_as_one() {
        let entry = parse_entry("drwxr-xr-x   2 root root        0 Aug 25 12:00 pldmgr");
        assert_eq!(entry.kind, Kind::Directory);
        assert_eq!(entry.name, "pldmgr");
    }

    /// The listing's own header is not an entry and is not an unreadable line.
    ///
    /// Every listing has one. Reporting it as something that could not be read would name
    /// it in every backup ever taken, which is how a real warning stops being read.
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

    /// A line that is not understood is kept and marked, because a listing that quietly
    /// drops what it could not read says a directory is empty when it is not.
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
