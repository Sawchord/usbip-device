use serde::{Deserialize, Serialize};

// TODO: Unlink commands
// TODO: Remove ssmarshal

#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsbIpHeader {
   command: u32,
   seqnum: u32,
   devid: u32,
}

pub enum UsbIpRequest {
   Cmd(UsbIpCmd, Vec<u8>),
}

impl UsbIpRequest {
   pub fn from_slice(data: &[u8]) -> Option<Self> {
      // Parse header
      if data.len() < 12 {
         log::warn!("received packet that is too short");
         return None;
      }

      let header: UsbIpHeader = match ssmarshal::deserialize(&data[..12]) {
         Ok(header) => header.0,
         _ => {
            log::warn!("failed to deserialize header");
            return None;
         }
      };

      log::debug!(
         "received request with seqnum {} for devid {}",
         header.seqnum,
         header.devid
      );

      match header.command {
         0x00000001 => {
            if data.len() < 48 {
               log::warn!("paket is to short to be a command");
               return None;
            }

            let command: UsbIpCmd = match ssmarshal::deserialize(&data[12..48]) {
               Ok(command) => command.0,
               _ => {
                  log::warn!("failed to deserialize command packet");
                  return None;
               }
            };

            log::info!("parsed a command request");
            Some(Self::Cmd(command, data[48..].to_vec()))
         }
         _ => {
            log::warn!(
               "received packet with unsupported command {}",
               header.command
            );
            return None;
         }
      }
   }
}

#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsbIpCmd {
   direction: u32,
   ep: u32,
   transfer_flags: u32,
   transfer_buffer_length: u32,
   start_frame: u32,
   number_of_packets: u32,
   interval_or_err_count: u32,
   setup: u64,
}

// TODO: Implement Buffer flags
// TODO: Implement buffer flag integrity check

pub struct UsbIpResponse {
   header: UsbIpHeader,
   cmd: UsbIpResponseCmd,
   data: Vec<u8>,
}

enum UsbIpResponseCmd {
   Cmd(UsbIpCmd),
}

impl UsbIpResponse {
   pub fn to_vec(self) -> Option<Vec<u8>> {
      let mut result = vec![];

      // Parse the header
      let mut header_buf = [0; 12];
      ssmarshal::serialize(&mut header_buf, &self.header).unwrap();
      result.extend_from_slice(&header_buf);

      // parse the command
      match self.cmd {
         UsbIpResponseCmd::Cmd(cmd) => {
            let mut cmd_buf = [0; 36];
            ssmarshal::serialize(&mut cmd_buf, &cmd).unwrap();
            result.extend_from_slice(&cmd_buf);
         }
      }

      // parse the data
      result.extend_from_slice(&self.data[..]);

      Some(result)
   }
}
