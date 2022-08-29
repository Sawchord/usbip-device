use crate::{
    cmd::{Direction, TransferFlags, UsbCmd, UsbIpHeader},
    debug::{DbgBuf, DbgEmpty},
    UsbIpError,
};
use std::{
    convert::TryInto,
    fmt::{Debug, Formatter, Result as FmtResult},
    io::{Error, ErrorKind, Read},
    net::TcpStream,
};

#[derive(Clone)]
pub struct UsbIpRequest {
    pub header: UsbIpHeader,
    pub cmd: UsbIpRequestCmd,
    pub data: Vec<u8>,
}

impl Debug for UsbIpRequest {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.debug_struct("UsbIpRequest")
            .field("header", &self.header)
            .field("cmd", &self.cmd)
            .field("data", &DbgBuf(&self.data))
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum UsbIpRequestCmd {
    Cmd(UsbIpCmdSubmit),
    Unlink(UsbIpCmdUnlink),
}

impl UsbIpRequest {
    pub fn read(reader: &mut TcpStream) -> Result<Self, Error> {
        // Read an parse header
        reader.set_nonblocking(true)?;
        let mut buf = [0; 48];
        match reader.read(&mut buf) {
            Ok(bytes_read) if bytes_read == 48 => (),
            Ok(0) => {
                return Err(Error::new(
                    ErrorKind::NotConnected,
                    Box::new(UsbIpError::ConnectionClosed),
                ))
            }
            Ok(bytes_read) => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    Box::new(UsbIpError::PkgTooShort(bytes_read)),
                ))
            }
            Err(err) => return Err(err),
        }
        reader.set_nonblocking(false)?;

        let header = UsbIpHeader::from_slice(&buf[0..20]);
        match header.command {
            UsbCmd::Request => {
                let cmd = UsbIpCmdSubmit::from_slice(&buf[20..48]);

                // Receive the URB if this is a OUT packet
                let data = if header.direction == Direction::OUT && cmd.transfer_buffer_length != 0
                {
                    // NOTE: Reading 0 bytes would still block the reader
                    let mut data = vec![0; cmd.transfer_buffer_length as usize];
                    reader.read_exact(&mut data)?;

                    data
                } else {
                    vec![]
                };

                //Ok(Self::Cmd(header, cmd, data))
                Ok(Self {
                    header,
                    cmd: UsbIpRequestCmd::Cmd(cmd),
                    data,
                })
            }
            UsbCmd::UnlinkRequest => {
                let unlink = UsbIpCmdUnlink::from_slice(&buf[20..24]);

                // NOTE: We do not expect to see urb data behind an unlink

                Ok(Self {
                    header,
                    cmd: UsbIpRequestCmd::Unlink(unlink),
                    data: vec![],
                })
            }
            _ => Err(Error::new(
                ErrorKind::InvalidInput,
                Box::new(UsbIpError::InvalidCommand(header.command as u16)),
            )),
        }
    }
}

#[derive(Clone)]
pub struct UsbIpCmdSubmit {
    pub transfer_flags: TransferFlags,
    pub transfer_buffer_length: i32,
    pub start_frame: i32,
    pub number_of_packets: i32,
    pub interval: i32,
    pub setup: [u8; 8],
}

impl Debug for UsbIpCmdSubmit {
    /// As `start_frame`, `number_of_packets` and `interval` are unused as of now,
    /// they are not being printed
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        // Only output setup bytes, if they are relevant
        let setup_dbg = DbgBuf(&self.setup);
        let setup: &dyn Debug = if self.setup != [0, 0, 0, 0, 0, 0, 0, 0] {
            &setup_dbg
        } else {
            &DbgEmpty
        };

        f.debug_struct("UsbIpCmdSubmit")
            .field("transfer_flags", &self.transfer_flags)
            .field("transfer_buffer_length", &self.transfer_buffer_length)
            .field("setup", &setup)
            .finish()
    }
}

impl UsbIpCmdSubmit {
    fn from_slice(data: &[u8]) -> Self {
        Self {
            transfer_flags: TransferFlags::from_bits_truncate(u32::from_be_bytes(
                data[0..4].try_into().unwrap(),
            )),
            transfer_buffer_length: i32::from_be_bytes(data[4..8].try_into().unwrap()),
            start_frame: i32::from_be_bytes(data[8..12].try_into().unwrap()),
            number_of_packets: i32::from_be_bytes(data[12..16].try_into().unwrap()),
            interval: i32::from_be_bytes(data[16..20].try_into().unwrap()),
            setup: data[20..28].try_into().unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsbIpCmdUnlink {
    pub seqnum: u32,
}

impl UsbIpCmdUnlink {
    fn from_slice(data: &[u8]) -> Self {
        Self {
            seqnum: u32::from_be_bytes(data[0..4].try_into().unwrap()),
        }
    }
}
