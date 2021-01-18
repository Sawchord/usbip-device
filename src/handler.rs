use crate::{
   cmd::{UsbIpCmd, UsbIpHeader, UsbIpRequest, UsbIpResponse, UsbIpResponseCmd},
   op::{OpDeviceDescriptor, OpInterfaceDescriptor, OpRequest, OpResponse, OpResponseCommand},
   {UsbIpBus, UsbIpBusInner},
};
use std::{
   io::Write,
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
         let reset = self.bus.lock().reset;
         let response = match reset {
            // Handle Op list case
            // NOTE: Next line blocks, do not hold lock
            true => match OpRequest::read(&mut stream) {
               Some(op) => match handle_op(self.bus.lock(), op) {
                  Some(response) => response,
                  None => break,
               },
               None => break,
            },

            // Handle connected case
            // NOTE: Next line blocks, do not hold lock
            false => match UsbIpRequest::read(&mut stream) {
               Some(cmd) => match handle_cmd(self.bus.lock(), cmd) {
                  Some(response) => response,
                  None => break,
               },
               None => continue,
            },
         };

         stream.write(&response[..]).unwrap();
      }
   }
}

fn handle_op(
   //stream: &mut TcpStream,
   mut inner: MutexGuard<UsbIpBusInner>,
   op: OpRequest,
) -> Option<Vec<u8>> {
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

         Some(list_response.to_vec().unwrap())
      }
      OpRequest::ConnectDevice(header) => {
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
         log::info!("device is leaving reset state");
         inner.reset = false;

         Some(list_response.to_vec().unwrap())
      }
   }
}

fn handle_cmd(mut inner: MutexGuard<UsbIpBusInner>, cmd: UsbIpRequest) -> Option<Vec<u8>> {
   match cmd {
      UsbIpRequest::Cmd(header, cmd, data) => {
         // Get the endpoint and push packets to input
         let ep = inner.get_endpoint(cmd.ep as usize).ok()?;

         log::info!(
            "received cmd for dev {} endpoint {}, seqnum {}, data length {}",
            header.devid,
            cmd.ep,
            header.seqnum,
            data.len(),
         );

         // pass the data into the correct buffers
         for chunk in data.chunks(ep.out_ep.unwrap().max_packet_size as usize) {
            ep.out_buf.push_back(chunk.to_vec());
         }

         // read the data into an output buffer
         let mut output = vec![];
         let mut num_pkgs = 0;
         for pkg in ep.in_buf.drain(..) {
            num_pkgs += 1;
            output.extend_from_slice(&pkg);
         }

         // return packet

         // return packet
         let response = UsbIpResponse {
            header: UsbIpHeader {
               command: 0x0003,
               seqnum: header.seqnum,
               devid: header.devid,
            },
            cmd: UsbIpResponseCmd::Cmd(UsbIpCmd {
               // TODO: Check these settings
               direction: 0,
               ep: cmd.ep,
               transfer_flags: 0,
               transfer_buffer_length: output.len() as u32,
               start_frame: 0,
               number_of_packets: num_pkgs,
               interval_or_err_count: cmd.interval_or_err_count,
               setup: [0, 0, 0, 0, 0, 0, 0, 0],
            }),
            data: output,
         };

         Some(response.to_vec().unwrap())
      }
   }
}
