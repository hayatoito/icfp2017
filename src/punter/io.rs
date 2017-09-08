use punter::prelude::*;
use std;
use std::io::BufReader;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::Path;
use std::str;

fn read_n<R>(read: R, bytes_to_read: u64) -> PunterResult<Vec<u8>>
where
    R: Read,
{
    let mut buf = vec![];
    let mut chunk = read.take(bytes_to_read);
    chunk.read_to_end(&mut buf)?;
    Ok(buf)
}

fn read_json_message<R>(r: &mut R) -> PunterResult<String>
where
    R: BufRead,
{
    let mut buf = vec![];
    let size = r.read_until(':' as u8, &mut buf)?;
    debug!("{} bytes read", size);

    if size == 0 {
        return Err(PunterError::Io(std::io::ErrorKind::InvalidData.into()));
    }

    // Drop ":".
    let n_str = str::from_utf8(&buf[0..buf.len() - 1]).unwrap();
    debug!("n_str: {}", n_str);

    // message might contain leading space, such as \n

    let n: u64 = n_str.trim().parse()?;

    debug!("parsed n: {}", n);
    let buf = read_n(r, n)?;
    assert_eq!(buf.len(), n as usize);
    let s = String::from_utf8(buf)?;
    debug!("C <= S: {}", s);
    Ok(s)
}

#[test]
fn read_test() {
    let input_data = b"4:abcdefg";
    let mut reader = BufReader::new(&input_data[..]);
    let s = read_json_message(&mut reader).unwrap();
    assert_eq!(s, "abcd");
}

fn write_json_message<W>(w: &mut W, json: &str) -> std::io::Result<()>
where
    W: Write,
{
    debug!("C => S: {}", json);
    let message = format!("{}:{}", json.as_bytes().len(), json);
    // let message = format!("{}:{}\n", json.as_bytes().len(), json);
    w.write(message.as_bytes())?;
    w.flush().unwrap();
    Ok(())
}

pub struct OfflineIO<R, W> {
    read: BufReader<R>,
    stdout: W,
}

impl<R, W> OfflineIO<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(r: R, w: W) -> Self {
        OfflineIO {
            read: BufReader::new(r),
            stdout: w,
        }
    }

    pub fn read_json_message(&mut self) -> PunterResult<String> {
        read_json_message(&mut self.read)
    }

    pub fn write_json_message(&mut self, json: &str) -> std::io::Result<()> {
        write_json_message(&mut self.stdout, json)
    }
}

pub struct OnlineIO {
    stream: BufReader<TcpStream>,
}

impl OnlineIO {
    pub fn new(server_address: &str) -> OnlineIO {
        debug!("server_address: {}", server_address);
        let stream = TcpStream::connect(server_address).unwrap();
        debug!("connected");
        OnlineIO { stream: BufReader::new(stream) }
    }

    pub fn read_json_message(&mut self) -> PunterResult<String> {
        read_json_message(&mut self.stream)
    }

    pub fn write_json_message(&mut self, json: &str) -> std::io::Result<()> {
        write_json_message(self.stream.get_mut(), json)
    }
}

use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct ChildIO {
    child: Child,
    write: ChildStdin,
    read: BufReader<ChildStdout>,
}

impl ChildIO {
    pub fn new<P: AsRef<Path>>(p: P) -> Self {
        let mut child = Command::new(p.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var("MY_ICFP2017_BOT_PRINT_STDERR").is_ok() {
                Stdio::inherit()
            } else {
                Stdio::piped()
            })
            .spawn()
            .expect("failed to execute child");

        // Use Option::take() to destruct child's ownership.
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        ChildIO {
            child: child,
            write: stdin,
            read: BufReader::new(stdout),
        }
    }

    pub fn read_json_message(&mut self) -> PunterResult<String> {
        read_json_message(&mut self.read)
    }

    pub fn write_json_message(&mut self, json: &str) -> std::io::Result<()> {
        write_json_message(&mut self.write, json)
    }

    pub fn wait(self) -> PunterResult<()> {
        let output = self.child.wait_with_output()?;
        if !output.stderr.is_empty() {
            debug!("stderr: {}", String::from_utf8(output.stderr)?);
        }
        Ok(())
    }
}
