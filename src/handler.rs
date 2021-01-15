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

         let inner = self.bus.lock();
         let response = match inner.reset {
            true => match OpRequest::from_slice(&buf[..bytes_read]) {
               Some(op) => handle_op(inner, op),
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

fn handle_op(mut inner: MutexGuard<UsbIpBusInner>, op: OpRequest) -> Vec<u8> {
   match op {
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

         list_response.to_vec().unwrap()
      }
      OpRequest::ConnectDevice(header, _bus_id) => {
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
         inner.reset = false;

         list_response.to_vec().unwrap()
      }
   }
}

fn handle_cmd(_inner: MutexGuard<UsbIpBusInner>, _cmd: UsbIpRequest) -> Vec<u8> {
   todo!()
}
