use crate::{
   cmd::UsbIpRequest,
   op::{OpDeviceDescriptor, OpInterfaceDescriptor, OpRequest, OpResponse, OpResponseCommand},
   {UsbIpBus, UsbIpBusInner},
};
use std::{
   io::{Read, Write},
   net::{TcpListener, TcpStream},
   sync::MutexGuard,
};

#[derive(Debug)]
pub struct SocketHandler {
   bus: UsbIpBus,
   listener: TcpListener,
}

impl SocketHandler {
   pub fn run(bus: UsbIpBus) {
      let mut handler = Self {
         bus,
         listener: TcpListener::bind(("127.0.0.1", 3240)).unwrap(),
      };

      log::info!("starting tcp listener thread");
      std::thread::spawn(move || {
         handler.listen();
      });
   }

   fn listen(&mut self) {
      loop {
         match self.listener.accept() {
            Ok(stream) => {
               log::info!("accepted connection from {}", stream.1);
               self.handle_connection(stream.0)
            }
            Err(e) => {
               log::warn!("error {:?} while listening for stream", e);
            }
         }
      }
   }

   fn handle_connection(&mut self, mut stream: TcpStream) {
      loop {
         let mut buf = [0; 4096];
         // NOTE: This call blocks. We must not hold the lock while calling it
         let bytes_read = stream.read(&mut buf).unwrap();
         log::debug!("read {} bytes from socket", bytes_read);

         let mut inner = self.bus.lock();
         if bytes_read == 0 {
            inner.reset = true;
            log::info!("connection closed, device entering reset state");
            break;
         }

         let response = match inner.reset {
            true => match OpRequest::from_slice(&buf[..bytes_read]) {
               Some(op) => match handle_op(&mut stream, inner, op) {
                  Some(op) => op,
                  None => break,
               },
               None => continue,
            },
            false => match UsbIpRequest::from_slice(&buf[..bytes_read]) {
               Some(cmd) => handle_cmd(inner, cmd),
               None => continue,
            },
         };

         // TODO: Handle closed stream
         stream.write(&response[..]).unwrap();
      }
   }
}

fn handle_op(
   stream: &mut TcpStream,
   mut inner: MutexGuard<UsbIpBusInner>,
   op: OpRequest,
) -> Option<Vec<u8>> {
   match op {
      // FIXME: List devices has a bug, probably a field is missing somewhere
      OpRequest::ListDevices(header) => {
         let list_response = OpResponse {
            version: header.version,
            path: "/sys/devices/pci0000:00/0000:00:01.2/usb1/1-1".to_string(),
            bus_id: "1-1".to_string(),
            descriptor: OpDeviceDescriptor {
               busnum: 1,
               devnum: 2,
               speed: 2,

               // These values should be settable via configuration
               vendor: 0x1111,
               product: 0x1010,
               bcd_device: 0,
               device_class: 0,
               device_subclass: 0,
               device_protocol: 0,
               configuration_value: 0,

               // These are fixed for this implementation
               num_configurations: 1,
               num_interfaces: 1,
            },
            cmd: OpResponseCommand::ListDevices(OpInterfaceDescriptor {
               // TODO: Make these settabel
               interface_class: 0,
               interface_subclass: 0,
               interface_protocol: 0,
               padding: 0,
            }),
         };

         Some(list_response.to_vec().unwrap())
      }
      OpRequest::ConnectDevice(header) => {
         // Reveice the bus is packet
         let mut data = [0; 4096];

         // NOTE: We block here while holding the lock
         // Maybe we should release lock in the meantime
         stream.read(&mut data).unwrap();
         if data.len() == 32 {
            log::warn!("packet has length of {}, expected 32", data[8..].len());
            return None;
         }

         let bus_id = match std::str::from_utf8(&data) {
            Ok(data) => data.trim_matches(char::from(0)),
            _ => {
               log::warn!("failed to read usb-bus id");
               return None;
            }
         };

         log::debug!("connect request for bus id {}", bus_id);

         let list_response = OpResponse {
            version: header.version,
            path: "/sys/devices/pci0000:00/0000:00:01.2/usb1/1-1".to_string(),
            bus_id: "1-1".to_string(),
            descriptor: OpDeviceDescriptor {
               busnum: 1,
               devnum: 2,
               speed: 2,

               // These values should be settable via configuration
               vendor: 0x1111,
               product: 0x1010,
               bcd_device: 0,
               device_class: 0,
               device_subclass: 0,
               device_protocol: 0,
               configuration_value: 0,

               // These are fixed for this implementation
               num_configurations: 1,
               num_interfaces: 1,
            },
            cmd: OpResponseCommand::ConnectDevice,
         };

         // Set the inner value to not reset, because we have connected the device
         log::info!("device is leaving ready state");
         inner.reset = false;

         Some(list_response.to_vec().unwrap())
      }
   }
}

fn handle_cmd(_inner: MutexGuard<UsbIpBusInner>, _cmd: UsbIpRequest) -> Vec<u8> {
   todo!()
}
