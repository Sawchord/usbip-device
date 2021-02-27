use crate::UsbIpError;
use std::{
   convert::TryInto,
   io::{Error, ErrorKind, Read},
   net::TcpStream,
};

/// The command type of the Urb
#[derive(Debug, Clone, Copy)]
pub enum UsbCmd {
   Request,
   UnlinkRequest,
   Response,
   UnlinkResponse,
}

impl UsbCmd {
   pub fn to_u32(self) -> u32 {
      match self {
         UsbCmd::Request => 1,
         UsbCmd::UnlinkRequest => 2,
         UsbCmd::Response => 3,
         UsbCmd::UnlinkResponse => 4,
      }
   }

   pub fn try_from_u32(num: u32) -> Option<Self> {
      match num {
         1 => Some(UsbCmd::Request),
         2 => Some(UsbCmd::UnlinkRequest),
         3 => Some(UsbCmd::Response),
         4 => Some(UsbCmd::UnlinkResponse),
         _ => None,
      }
   }
}

#[derive(Debug, Clone)]
pub struct UsbIpHeader {
   pub command: UsbCmd,
   pub seqnum: u32,
   pub devid: u32,
   pub direction: Direction,
   pub ep: u32,
}

impl UsbIpHeader {
   fn to_array(&self) -> [u8; 20] {
      let mut result = [0; 20];

      result[0..4].copy_from_slice(&self.command.to_u32().to_be_bytes());
      result[4..8].copy_from_slice(&self.seqnum.to_be_bytes());
      result[8..12].copy_from_slice(&self.devid.to_be_bytes());
      result[12..16].copy_from_slice(&self.direction.bits().to_be_bytes());
      result[16..20].copy_from_slice(&self.ep.to_be_bytes());

      result
   }

   fn from_slice(data: &[u8]) -> Self {
      Self {
         command: UsbCmd::try_from_u32(u32::from_be_bytes(data[0..4].try_into().unwrap())).unwrap(),
         seqnum: u32::from_be_bytes(data[4..8].try_into().unwrap()),
         devid: u32::from_be_bytes(data[8..12].try_into().unwrap()),
         direction: Direction::from_bits_truncate(u32::from_be_bytes(
            data[12..16].try_into().unwrap(),
         )),
         ep: u32::from_be_bytes(data[16..20].try_into().unwrap()),
      }
   }
}

#[derive(Debug, Clone)]
pub struct UsbIpRequest {
   pub header: UsbIpHeader,
   pub cmd: UsbIpRequestCmd,
   pub data: Vec<u8>,
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
            let data = if header.direction == Direction::OUT && cmd.transfer_buffer_length != 0 {
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

#[derive(Debug, Clone)]
pub struct UsbIpCmdSubmit {
   pub transfer_flags: TransferFlags,
   pub transfer_buffer_length: i32,
   pub start_frame: i32,
   pub number_of_packets: i32,
   pub interval: i32,
   pub setup: [u8; 8],
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

bitflags::bitflags! {
   pub struct TransferFlags: u32 {
      const SHORT_NOT_OK = 0x00000001;
      const ISO_ASAP = 0x00000002;
      const NO_TRANSFER_DMA_MAP = 0x00000004;
      const ZERO_PACKET = 0x00000040;
      const NO_INTERRUPT = 0x00000080;
      const FREE_BUFFER = 0x00000100;
      const DIR_MASK = 0x00000200;
   }
}

bitflags::bitflags! {
   pub struct Direction: u32 {
      const OUT = 0x0000000;
      const IN = 0x0000001;
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

// TODO: Make header command field unsettable and set it by cmd
#[derive(Debug, Clone)]
pub struct UsbIpResponse {
   pub header: UsbIpHeader,
   pub cmd: UsbIpResponseCmd,
   pub data: Vec<u8>,
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

#[derive(Debug, Clone)]
pub struct UsbIpRetSubmit {
   pub status: i32,
   pub actual_length: i32,
   pub start_frame: i32,
   pub number_of_packets: i32,
   pub error_count: i32,
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
