use crate::UsbIpError;
use std::{
   convert::TryInto,
   io::{Error, ErrorKind, Read},
   net::TcpStream,
};

// TODO: Unlink commands

#[repr(C)]
#[derive(Debug, Clone)]
pub struct UsbIpHeader {
   pub command: u32,
   pub seqnum: u32,
   pub devid: u32,
   pub direction: u32,
   pub ep: u32,
}

impl UsbIpHeader {
   fn to_array(&self) -> [u8; 20] {
      let mut result = [0; 20];

      result[0..4].copy_from_slice(&self.command.to_be_bytes());
      result[4..8].copy_from_slice(&self.seqnum.to_be_bytes());
      result[8..12].copy_from_slice(&self.devid.to_be_bytes());
      result[12..16].copy_from_slice(&self.direction.to_be_bytes());
      result[16..20].copy_from_slice(&self.ep.to_be_bytes());

      result
   }

   fn from_slice(data: &[u8]) -> Self {
      Self {
         command: u32::from_be_bytes(data[0..4].try_into().unwrap()),
         seqnum: u32::from_be_bytes(data[4..8].try_into().unwrap()),
         devid: u32::from_be_bytes(data[8..12].try_into().unwrap()),
         direction: u32::from_be_bytes(data[12..16].try_into().unwrap()),
         ep: u32::from_be_bytes(data[16..20].try_into().unwrap()),
      }
   }
}

pub enum UsbIpRequest {
   Cmd(UsbIpHeader, UsbIpCmdSubmit, Vec<u8>),
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

      log::debug!(
         "received request with seqnum {} for devid {}",
         header.seqnum,
         header.devid
      );

      match header.command {
         0x00000001 => {
            let command = UsbIpCmdSubmit::from_slice(&buf[20..48]);

            // Receive the URB if this is a OUT packet
            let urb_buf = if header.direction == 0 && command.transfer_buffer_length != 0 {
               // NOTE: Reading 0 bytes would still block the reader
               let mut urb_buf = vec![0; command.transfer_buffer_length as usize];
               reader.read_exact(&mut urb_buf)?;

               urb_buf
            } else {
               vec![]
            };

            Ok(Self::Cmd(header, command, urb_buf))
         }
         _ => Err(Error::new(
            ErrorKind::InvalidInput,
            Box::new(UsbIpError::InvalidCommand(header.command as u16)),
         )),
      }
   }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct UsbIpCmdSubmit {
   pub transfer_flags: u32,
   pub transfer_buffer_length: i32,
   pub start_frame: i32,
   pub number_of_packets: i32,
   pub interval: i32,
   pub setup: [u8; 8],
}

impl UsbIpCmdSubmit {
   fn from_slice(data: &[u8]) -> Self {
      Self {
         transfer_flags: u32::from_be_bytes(data[0..4].try_into().unwrap()),
         transfer_buffer_length: i32::from_be_bytes(data[4..8].try_into().unwrap()),
         start_frame: i32::from_be_bytes(data[8..12].try_into().unwrap()),
         number_of_packets: i32::from_be_bytes(data[12..16].try_into().unwrap()),
         interval: i32::from_be_bytes(data[16..20].try_into().unwrap()),
         setup: data[20..28].try_into().unwrap(),
      }
   }
}

// TODO: Implement transfer flags

#[derive(Debug, Clone)]
pub struct UsbIpResponse {
   pub header: UsbIpHeader,
   pub cmd: UsbIpResponseCmd,
   pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum UsbIpResponseCmd {
   Cmd(UsbIpRetSubmit),
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
      }

      // parse the data
      result.extend_from_slice(&self.data[..]);

      Some(result)
   }
}

#[repr(C)]
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
