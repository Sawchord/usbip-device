use std::convert::TryInto;

// TODO: Unlink commands
// TODO: Remove ssmarshal

#[repr(C)]
#[derive(Debug, Clone)]
pub struct UsbIpHeader {
   command: u32,
   seqnum: u32,
   devid: u32,
}

impl UsbIpHeader {
   fn to_array(&self) -> [u8; 12] {
      let mut result = [0; 12];

      result[0..4].copy_from_slice(&self.command.to_be_bytes());
      result[4..8].copy_from_slice(&self.seqnum.to_be_bytes());
      result[8..12].copy_from_slice(&self.devid.to_be_bytes());

      result
   }

   fn from_slice(data: &[u8]) -> Self {
      Self {
         command: u32::from_be_bytes(data[0..4].try_into().unwrap()),
         seqnum: u32::from_be_bytes(data[4..8].try_into().unwrap()),
         devid: u32::from_be_bytes(data[8..12].try_into().unwrap()),
      }
   }
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

      let header = UsbIpHeader::from_slice(&data[..12]);

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

            let command = UsbIpCmd::from_slice(&data[12..48]);

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
#[derive(Debug, Clone)]
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

impl UsbIpCmd {
   fn to_array(&self) -> [u8; 36] {
      let mut result = [0; 36];

      result[0..4].copy_from_slice(&self.direction.to_be_bytes());
      result[4..8].copy_from_slice(&self.ep.to_be_bytes());
      result[8..12].copy_from_slice(&self.transfer_flags.to_be_bytes());
      result[12..16].copy_from_slice(&self.transfer_buffer_length.to_be_bytes());
      result[16..20].copy_from_slice(&self.start_frame.to_be_bytes());
      result[20..24].copy_from_slice(&self.number_of_packets.to_be_bytes());
      result[24..28].copy_from_slice(&self.interval_or_err_count.to_be_bytes());
      result[28..36].copy_from_slice(&self.setup.to_be_bytes());

      result
   }

   fn from_slice(data: &[u8]) -> Self {
      Self {
         direction: u32::from_be_bytes(data[0..4].try_into().unwrap()),
         ep: u32::from_be_bytes(data[4..8].try_into().unwrap()),
         transfer_flags: u32::from_be_bytes(data[8..12].try_into().unwrap()),
         transfer_buffer_length: u32::from_be_bytes(data[12..16].try_into().unwrap()),
         start_frame: u32::from_be_bytes(data[16..20].try_into().unwrap()),
         number_of_packets: u32::from_be_bytes(data[20..24].try_into().unwrap()),
         interval_or_err_count: u32::from_be_bytes(data[24..28].try_into().unwrap()),
         setup: u64::from_be_bytes(data[28..36].try_into().unwrap()),
      }
   }
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
      result.extend_from_slice(&self.header.to_array());

      // parse the command
      match self.cmd {
         UsbIpResponseCmd::Cmd(cmd) => {
            result.extend_from_slice(&cmd.to_array());
         }
      }

      // parse the data
      result.extend_from_slice(&self.data[..]);

      Some(result)
   }
}
