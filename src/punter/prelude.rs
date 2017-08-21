use serde_json;
use std;

quick_error! {
    #[derive(Debug)]
    pub enum PunterError {
        Io(err: std::io::Error) {
            from()
        }
        ParseInt(err: std::num::ParseIntError) {
            from()
        }
        FromUtf8(err: std::string::FromUtf8Error) {
            from()
        }
        Json(err: serde_json::Error) {
            from()
        }
    }
}

pub type PunterResult<T> = Result<T, PunterError>;

pub type PunterId = usize;
pub type Nat = u64;

pub type SiteId = Nat;
