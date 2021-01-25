use crate::{
   cmd::{
      TransferFlags, UsbIpHeader, UsbIpRequest, UsbIpRequestCmd, UsbIpResponse, UsbIpResponseCmd,
      UsbIpRetSubmit,
   },
   op::{OpDeviceDescriptor, OpInterfaceDescriptor, OpRequest, OpResponse, OpResponseCommand},
   UsbIpBusInner,
};
use std::{
   io::{ErrorKind, Write},
   net::{TcpListener, TcpStream},
};
use usb_device::{endpoint::EndpointType, UsbError};

#[derive(Debug)]
pub struct SocketHandler {
   listener: TcpListener,
   connection: Option<TcpStream>,
}

const DEVICE_SPEED: u32 = 1;

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
   pub fn handle_socket(&mut self) {
      match self.handler.connection {
         // If not connected, listen for new connections
         None => match self.handler.listener.accept() {
            Ok((connection, addr)) => {
               log::info!("new connection from: {}", addr);
               self.handler.connection = Some(connection)
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => (),
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
                  self.handle_cmd(cmd);
               }
            }
         }
      }
   }

   pub fn try_send_pending(&mut self, ep_addr: usize) {
      let ep = match self.get_endpoint(ep_addr) {
         Ok(ep) => ep,
         Err(_) => {
            log::warn!("request to send on uninitalized endpoint");
            return;
         }
      };

      if !ep.is_rts() {
         return;
      }

      let (header, cmd, _data) = match ep.pending_ins.pop_front() {
         Some(urb) => urb,
         None => return,
      };
      let bytes_requested = cmd.transfer_buffer_length;

      let ep_in = match ep.get_in() {
         Ok(ep_in) => ep_in,
         Err(UsbError::InvalidEndpoint) => return,
         Err(e) => panic!("unexpected error {:?} while processing in packet", e),
      };

      // Read data from the packet buffer into the output buffer
      // We must be careful to not send more bytes than requested
      let mut out_buf = vec![];
      while let Some(data) = ep_in.data.pop_front() {
         let bytes_left = bytes_requested as usize - out_buf.len();
         let bytes_to_read = usize::min(data.len(), bytes_left);

         out_buf.extend_from_slice(&data[..bytes_to_read]);

         if bytes_to_read != data.len() {
            assert_eq!(out_buf.len(), bytes_requested as usize);
            ep_in.data.push_front(data[bytes_to_read..].to_vec());
            break;
         }
      }

      // TODO: Error if exact read was requested and out_buf.len() smaller than bytes_requested

      let response = UsbIpResponse {
         header: UsbIpHeader {
            command: 0x0003,
            seqnum: header.seqnum,
            devid: 2,
            direction: 1,
            ep: ep_addr as u32,
         },
         cmd: UsbIpResponseCmd::Cmd(UsbIpRetSubmit {
            // TODO: Check these settings
            status: 0,
            actual_length: out_buf.len() as i32,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
         }),
         data: out_buf,
      };
      log::info!(
         "header: {:?}, cmd: {:?}. data: {:?}",
         response.header,
         response.cmd,
         response.data
      );

      self
         .handler
         .connection
         .as_mut()
         .unwrap()
         .write_all(&response.to_vec().unwrap())
         .unwrap();
   }

   /// Handles an incomming op packet, sends out the corresponding response
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
                  speed: DEVICE_SPEED,

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
                  // TODO: Make these setable
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
               .write_all(&list_response.to_vec().unwrap())
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
                  speed: DEVICE_SPEED,

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
               .write_all(&list_response.to_vec().unwrap())
               .unwrap();
         }
      }
   }

   fn handle_cmd(&mut self, request: UsbIpRequest) {
      match request.cmd {
         UsbIpRequestCmd::Cmd(cmd) => {
            log::info!(
               "header: {:?}, cmd: {:?}, data: {:?}",
               request.header,
               cmd,
               request.data
            );

            // Get the endpoint
            let ep = match self.get_endpoint(request.header.ep as usize) {
               Ok(ep) => ep,
               Err(err) => {
                  log::warn!("reveiced message for unimplemented endpoint {:?}", err);
                  return;
               }
            };

            // check wether we have a setup packet
            // NOTE: This assumes the control endpoints have no URBs pending
            if cmd.setup != [0, 0, 0, 0, 0, 0, 0, 0] {
               log::info!("setup was requested");
               ep.get_out().unwrap().data.push_back(cmd.setup.to_vec());
               ep.setup_flag = true;

               // Push this in packet to the front such that it is services first
               // if header.direction == 1 {
               //    ep.pending_ins.push_front((header, cmd, data));
               //    return;
               // }
            }

            match request.header.direction {
               0 => {
                  let ep_out = ep.get_out().unwrap();

                  // pass the data into the correct buffers
                  for chunk in request.data.chunks(ep_out.max_packet_size as usize) {
                     ep_out.data.push_back(chunk.to_vec());
                  }

                  if cmd.transfer_flags.contains(TransferFlags::ZERO_PACKET)
                     && ep_out.ty == EndpointType::Bulk
                  {
                     ep_out.data.push_back(vec![]);
                  }

                  self.ack_out(request.header.ep, request.header.seqnum);
               }
               1 => {
                  let ep_addr = request.header.ep;
                  ep.pending_ins
                     .push_back((request.header, cmd, request.data));
                  self.try_send_pending(ep_addr as usize);
               }
               _ => panic!(),
            }
         }
      }
   }

   fn ack_out(&mut self, ep: u32, seqnum: u32) {
      let response = UsbIpResponse {
         header: UsbIpHeader {
            command: 0x0003,
            seqnum: seqnum,
            devid: 2,
            direction: 1,
            ep,
         },
         cmd: UsbIpResponseCmd::Cmd(UsbIpRetSubmit {
            status: 0,
            actual_length: 0,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
         }),
         data: vec![],
      };
      log::info!(
         "header: {:?}, cmd: {:?}. data: {:?}",
         response.header,
         response.cmd,
         response.data
      );

      self
         .handler
         .connection
         .as_mut()
         .unwrap()
         .write_all(&response.to_vec().unwrap())
         .unwrap();
   }
}
