use crate::UsbIpError;
use std::{
    convert::TryInto,
    io::{Error, ErrorKind, Read},
    net::TcpStream,
};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct OpHeader {
    pub version: u16,
    pub command: u16,
    pub status: u32,
}

impl OpHeader {
    fn to_array(&self) -> [u8; 8] {
        let mut result = [0; 8];

        result[0..2].copy_from_slice(&self.version.to_be_bytes());
        result[2..4].copy_from_slice(&self.command.to_be_bytes());
        result[4..8].copy_from_slice(&self.status.to_be_bytes());

        result
    }

    fn from_slice(data: &[u8]) -> Self {
        Self {
            version: u16::from_be_bytes(data[0..2].try_into().unwrap()),
            command: u16::from_be_bytes(data[2..4].try_into().unwrap()),
            status: u32::from_be_bytes(data[4..8].try_into().unwrap()),
        }
    }
}

pub enum OpRequest {
    ListDevices(OpHeader),
    ConnectDevice(OpHeader),
}

impl OpRequest {
    pub fn read(reader: &mut TcpStream) -> Result<Self, Error> {
        // Receive the header
        reader.set_nonblocking(true)?;
        let mut header_buf = [0; 8];
        match reader.read(&mut header_buf) {
            Ok(bytes_read) if bytes_read == 8 => (),
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

        // Parse the header
        let header = OpHeader::from_slice(&header_buf);

        // Check status
        if header.status != 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                Box::new(UsbIpError::StatusNotOk(header.status)),
            ));
        }

        log::debug!("request version is {}", header.version);

        // Dispatch on command
        match header.command {
            0x8005 => {
                log::info!("received request to list devices");
                Ok(Self::ListDevices(header))
            }
            0x8003 => {
                let mut bus_id_buf = [0; 32];
                reader.read_exact(&mut bus_id_buf)?;

                let bus_id = match std::str::from_utf8(&bus_id_buf) {
                    Ok(data) => data.trim_matches(char::from(0)).to_string(),
                    Err(err) => {
                        return Err(Error::new(ErrorKind::InvalidInput, err));
                    }
                };

                log::info!("received request to connect device {}", bus_id);
                Ok(Self::ConnectDevice(header))
            }
            _ => Err(Error::new(
                ErrorKind::InvalidInput,
                Box::new(UsbIpError::InvalidCommand(header.command)),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpResponse {
    pub version: u16,
    pub path: String,
    pub bus_id: String,
    pub descriptor: OpDeviceDescriptor,
    pub cmd: OpResponseCommand,
}

#[derive(Debug, Clone)]
pub enum OpResponseCommand {
    ListDevices(OpInterfaceDescriptor),
    ConnectDevice,
}

impl OpResponse {
    pub fn to_vec(&self) -> Option<Vec<u8>> {
        let mut result = vec![];

        // Build and serialize the header
        let reply: u16 = match self.cmd {
            OpResponseCommand::ListDevices(_) => 0x0005,
            OpResponseCommand::ConnectDevice => 0x0003,
        };

        let header = OpHeader {
            version: self.version,
            command: reply,
            status: 0,
        };

        result.extend_from_slice(&header.to_array());

        match self.cmd {
            // This implementation can only ever export one device,
            // therefore number of exported devices is fixed
            OpResponseCommand::ListDevices(_) => result.extend_from_slice(&[0, 0, 0, 1]),
            OpResponseCommand::ConnectDevice => (),
        };

        // Serialize path
        let str_len = self.path.as_bytes().len();
        if str_len > 256 {
            log::warn!("path is longer than 256 bytes");
            return None;
        }

        let mut path_buf = [0; 256];
        path_buf[..str_len].copy_from_slice(self.path.as_bytes());
        result.extend_from_slice(&path_buf);

        // Serialize bus_id
        let str_len = self.bus_id.as_bytes().len();
        if str_len > 32 {
            log::warn!("bus_id is longr than 32 bytes");
            return None;
        }

        let mut bus_id_buf = [0; 32];
        bus_id_buf[..str_len].copy_from_slice(self.bus_id.as_bytes());
        result.extend_from_slice(&bus_id_buf);

        // Serialize the Op Desciptor
        result.extend_from_slice(&self.descriptor.to_array());

        // If exists, serialize the interface descriptor
        if let OpResponseCommand::ListDevices(ref interface) = self.cmd {
            result.extend_from_slice(&interface.to_array());
        }

        Some(result)
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct OpDeviceDescriptor {
    pub busnum: u32,
    pub devnum: u32,
    pub speed: u32,
    pub vendor: u16,
    pub product: u16,
    pub bcd_device: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub configuration_value: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
}

impl OpDeviceDescriptor {
    fn to_array(&self) -> [u8; 24] {
        let mut result = [0; 24];

        result[0..4].copy_from_slice(&self.busnum.to_be_bytes());
        result[4..8].copy_from_slice(&self.devnum.to_be_bytes());
        result[8..12].copy_from_slice(&self.speed.to_be_bytes());

        result[12..14].copy_from_slice(&self.vendor.to_be_bytes());
        result[14..16].copy_from_slice(&self.product.to_be_bytes());
        result[16..18].copy_from_slice(&self.bcd_device.to_be_bytes());

        result[18..24].copy_from_slice(&[
            self.device_class,
            self.device_subclass,
            self.device_protocol,
            self.configuration_value,
            self.num_configurations,
            self.num_interfaces,
        ]);

        result
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct OpInterfaceDescriptor {
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub padding: u8,
}

impl OpInterfaceDescriptor {
    fn to_array(&self) -> [u8; 4] {
        [
            self.interface_class,
            self.interface_subclass,
            self.interface_protocol,
            self.padding,
        ]
    }
}
