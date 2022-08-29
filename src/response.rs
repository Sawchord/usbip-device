use crate::{cmd::UsbIpHeader, debug::DbgBuf};
use std::fmt::{Debug, Formatter, Result as FmtResult};

#[derive(Clone)]
pub struct UsbIpResponse {
    pub header: UsbIpHeader,
    pub cmd: UsbIpResponseCmd,
    pub data: Vec<u8>,
}

impl Debug for UsbIpResponse {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.debug_struct("UsbIpResponse")
            .field("header", &self.header)
            .field("cmd", &self.cmd)
            .field("data", &DbgBuf(&self.data))
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum UsbIpResponseCmd {
    Cmd(UsbIpRetSubmit),
    Unlink(UsbIpRetUnlink),
}

impl UsbIpResponse {
    pub fn to_vec(&self) -> Option<Vec<u8>> {
        let mut result = vec![];

        // Parse the header
        result.extend_from_slice(&self.header.to_array());

        // parse the command
        match self.cmd {
            UsbIpResponseCmd::Cmd(ref cmd) => {
                result.extend_from_slice(&cmd.to_array());
            }
            UsbIpResponseCmd::Unlink(ref unlink) => {
                result.extend_from_slice(&unlink.to_array());
            }
        }

        // parse the data
        result.extend_from_slice(&self.data[..]);

        Some(result)
    }
}

#[derive(Clone)]
pub struct UsbIpRetSubmit {
    pub status: i32,
    pub actual_length: i32,
    pub start_frame: i32,
    pub number_of_packets: i32,
    pub error_count: i32,
}

impl Debug for UsbIpRetSubmit {
    /// As `start_frame`, `number_of_packets` and `error_count` are unused as of now,
    /// they are not being printed
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.debug_struct("UsbIpRetSubmit")
            .field("status", &self.status)
            .field("actual_length", &self.actual_length)
            .finish()
    }
}

impl UsbIpRetSubmit {
    fn to_array(&self) -> [u8; 28] {
        let mut result = [0; 28];

        result[0..4].copy_from_slice(&self.status.to_be_bytes());
        result[4..8].copy_from_slice(&self.actual_length.to_be_bytes());
        result[8..12].copy_from_slice(&self.start_frame.to_be_bytes());
        result[12..16].copy_from_slice(&self.number_of_packets.to_be_bytes());
        result[16..20].copy_from_slice(&self.error_count.to_be_bytes());

        result
    }
}

#[derive(Debug, Clone)]
pub struct UsbIpRetUnlink {
    pub status: u32,
}

impl UsbIpRetUnlink {
    fn to_array(&self) -> [u8; 28] {
        let mut result = [0; 28];
        result[0..4].copy_from_slice(&self.status.to_be_bytes());
        result
    }
}
