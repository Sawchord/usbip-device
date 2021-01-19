use crate::{
   cmd::{UsbIpCmd, UsbIpHeader, UsbIpRequest, UsbIpResponse, UsbIpResponseCmd},
   op::{OpDeviceDescriptor, OpInterfaceDescriptor, OpRequest, OpResponse, OpResponseCommand},
   UsbIpBusInner,
};
use std::{
   io::{ErrorKind, Write},
   net::{TcpListener, TcpStream},
};

#[derive(Debug)]
pub struct SocketHandler {
   listener: TcpListener,
   connection: Option<TcpStream>,
}

impl SocketHandler {
   pub fn new() -> Self {
      let listener = TcpListener::bind(("127.0.0.1", 3240)).unwrap();
      listener.set_nonblocking(true).unwrap();
      Self {
         listener,
         connection: None,
      }
   }
}

impl UsbIpBusInner {
   // TODO: Return UsbError in result
   pub fn handle_out(&mut self) {
      match self.handler.connection {
         // If not connected, listen for new connections
         None => match self.handler.listener.accept() {
            Ok((connection, addr)) => {
               log::info!("new connection from: {}", addr);
               self.handler.connection = Some(connection)
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return,
            Err(err) => panic!("unexpected error: {}", err),
         },
         // If connected, receive the data
         Some(ref mut stream) => {
            match self.reset {
               // If in reset state, answer op msgs
               true => {
                  // in case of Op, we directly send a response here
                  let op = match OpRequest::read(stream) {
                     Ok(op) => op,
                     Err(err) if err.kind() == ErrorKind::WouldBlock => return,
                     Err(err) if err.kind() == ErrorKind::NotConnected => {
                        self.handler.connection = None;
                        return;
                     }
                     Err(err) => panic!("unexpected error {}", err),
                  };
                  self.handle_op(op);
               }
               // If not in reset state, expect commands
               false => {
                  let cmd = match UsbIpRequest::read(stream) {
                     Ok(cmd) => cmd,
                     Err(err) if err.kind() == ErrorKind::WouldBlock => return,
                     Err(err) if err.kind() == ErrorKind::NotConnected => {
                        // If the connection is no longer connected, return to initial state
                        self.reset = true;
                        self.handler.connection = None;
                        return;
                     }
                     Err(err) => panic!("unexpected error {}", err),
                  };
                  self.handle_cmd_out(cmd);
               }
            }
         }
      }
   }

   pub fn handle_in(&mut self) {
      for (ep_idx, ep) in self.endpoint.iter_mut().enumerate() {
         for output in ep.in_buf.drain(..) {
            ep.in_complete_flag = true;
            let response = UsbIpResponse {
               header: UsbIpHeader {
                  command: 0x0003,
                  seqnum: ep.seqnum,
                  devid: 2,
               },
               cmd: UsbIpResponseCmd::Cmd(UsbIpCmd {
                  // TODO: Check these settings
                  direction: 0,
                  ep: ep_idx as u32,
                  transfer_flags: 0,
                  transfer_buffer_length: output.len() as u32,
                  start_frame: 0,
                  number_of_packets: 1,
                  interval_or_err_count: 0,
                  setup: [0, 0, 0, 0, 0, 0, 0, 0],
               }),
               data: output,
            };
            self
               .handler
               .connection
               .as_mut()
               .unwrap()
               .write(&response.to_vec().unwrap())
               .unwrap();
         }
      }
   }

   /// Handles an incomming op packet, sends out the corresponding response
   // FIXME: Clean up this function
   fn handle_op(&mut self, op: OpRequest) {
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

            self
               .handler
               .connection
               .as_mut()
               .unwrap()
               .write(&list_response.to_vec().unwrap())
               .unwrap();
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
            self.reset = false;

            self
               .handler
               .connection
               .as_mut()
               .unwrap()
               .write(&list_response.to_vec().unwrap())
               .unwrap();
         }
      }
   }

   /// Handles an outbound cmd packet, appends packets if necessary
   /// Sends return message if necessary
   fn handle_cmd_out(&mut self, cmd: UsbIpRequest) {
      match cmd {
         UsbIpRequest::Cmd(header, cmd, data) => {
            log::info!(
               "received cmd for dev {} endpoint {}, seqnum {}, data length {}",
               header.devid,
               cmd.ep,
               header.seqnum,
               data.len(),
            );

            // Get the endpoint and push packets to input
            let ep = match self.get_endpoint(cmd.ep as usize) {
               Ok(ep) => ep,
               Err(err) => {
                  log::warn!("reveiced message for unimplemented endpoint {:?}", err);
                  return;
               }
            };

            if header.seqnum < ep.seqnum {
               log::warn!("received seqnum is too small");
            }
            ep.seqnum = header.seqnum;

            // check wether we have a setup packet
            if cmd.setup != [0, 0, 0, 0, 0, 0, 0, 0] {
               log::info!("setup was requested");
               ep.out_buf.push_back(cmd.setup.to_vec());
               ep.setup_flag = true;
            }

            // pass the data into the correct buffers
            for chunk in data.chunks(ep.out_ep.unwrap().max_packet_size as usize) {
               ep.out_buf.push_back(chunk.to_vec());
            }
         }
      }
   }
}
