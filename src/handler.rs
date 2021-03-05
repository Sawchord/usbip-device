use crate::{
   cmd::{
      Direction, TransferFlags, UsbCmd, UsbIpCmdSubmit, UsbIpCmdUnlink, UsbIpHeader, UsbIpRequest,
      UsbIpRequestCmd, UsbIpResponse, UsbIpResponseCmd, UsbIpRetSubmit, UsbIpRetUnlink,
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

// TODO: Allow settable device speed
const DEVICE_SPEED: u32 = 3;

impl SocketHandler {
   /// Create a new handler
   pub fn new() -> Self {
      let listener = TcpListener::bind(("127.0.0.1", 3240)).unwrap();
      listener.set_nonblocking(true).unwrap();
      Self {
         listener,
         connection: None,
      }
   }

   pub fn is_connected(&self) -> bool {
      self.connection.is_some()
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
                  self.handle_usbip_pkg(cmd);
               }
            }
         }
      }
   }

   pub fn try_send_pending(&mut self, ep_addr: usize) {
      let ep = match self.get_endpoint(ep_addr) {
         Ok(ep) => ep,
         Err(_) => return,
      };

      if !ep.is_rts() {
         return;
      }

      let (header, _cmd, _) = match ep.pending_ins.pop_front() {
         Some(urb) => urb,
         None => return,
      };
      //let bytes_requested = cmd.transfer_buffer_length;

      let ep_in = match ep.get_in() {
         Ok(ep_in) => ep_in,
         Err(UsbError::InvalidEndpoint) => return,
         Err(e) => panic!("unexpected error {:?} while processing in packet", e),
      };

      // Read data from the packet buffer into the output buffer
      // We must be careful to not send more bytes than requested
      // FIXME: Fix this up so it supports transfer_buffers smaller than waiting data
      let mut out_buf = vec![];
      while let Some(data) = ep_in.data.pop_front() {
         //let bytes_left = bytes_requested as usize - out_buf.len();
         //let bytes_to_read = usize::min(data.len(), bytes_left);

         //out_buf.extend_from_slice(&data[..bytes_to_read]);
         out_buf.extend_from_slice(&data);

         // if bytes_to_read != data.len() {
         //    assert_eq!(out_buf.len(), bytes_requested as usize);
         //    ep_in.data.push_front(data[bytes_to_read..].to_vec());
         //    break;
         // }
      }

      // After sending, the in_complete can be set
      ep.in_complete_flag = true;

      // TODO: Error if exact read was requested and out_buf.len() smaller than bytes_requested

      let response = UsbIpResponse {
         header: UsbIpHeader {
            command: UsbCmd::Response,
            seqnum: header.seqnum,
            devid: 2,
            direction: Direction::IN,
            ep: ep_addr as u32,
         },
         cmd: UsbIpResponseCmd::Cmd(UsbIpRetSubmit {
            status: 0,
            actual_length: out_buf.len() as i32,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
         }),
         data: out_buf,
      };
      log::debug!(
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

   fn handle_usbip_pkg(&mut self, request: UsbIpRequest) {
      log::debug!(
         "header: {:?}, cmd: {:?}, data: {:?}",
         request.header,
         request.cmd,
         request.data
      );

      match request.cmd {
         UsbIpRequestCmd::Unlink(unlink) => self.handle_unlink(request.header, unlink),
         UsbIpRequestCmd::Cmd(cmd) => self.handle_cmd(request.header, cmd, request.data),
      }
   }

   /// Handle a [`UsbIpCmdSubmit`] package
   fn handle_cmd(&mut self, header: UsbIpHeader, cmd: UsbIpCmdSubmit, data: Vec<u8>) {
      // Get the endpoint
      let ep = match self.get_endpoint(header.ep as usize) {
         Ok(ep) => ep,
         Err(err) => {
            log::warn!("reveiced message for unimplemented endpoint {:?}", err);
            return;
         }
      };

      // check wether we have a setup packet
      // NOTE: This assumes the control endpoints have no URBs pending
      if cmd.setup != [0, 0, 0, 0, 0, 0, 0, 0] {
         ep.get_out().unwrap().data.push_back(cmd.setup.to_vec());
         ep.setup_flag = true;
      }

      match header.direction {
         Direction::OUT => {
            let ep_out = ep.get_out().unwrap();

            // pass the data into the correct buffers
            for chunk in data.chunks(ep_out.max_packet_size as usize) {
               ep_out.data.push_back(chunk.to_vec());
            }

            if cmd.transfer_flags.contains(TransferFlags::ZERO_PACKET)
               && ep_out.ty == EndpointType::Bulk
            {
               ep_out.data.push_back(vec![]);
            }

            self.ack_cmd_out(header.ep, header.seqnum);
         }
         Direction::IN => {
            let ep_addr = header.ep;
            ep.pending_ins.push_back((header, cmd, data));
            self.try_send_pending(ep_addr as usize);
         }
         _ => panic!(),
      }
   }

   /// Send an acknowledgement after recieving a cmd out package.
   fn ack_cmd_out(&mut self, ep: u32, seqnum: u32) {
      let response = UsbIpResponse {
         header: UsbIpHeader {
            command: UsbCmd::Response,
            seqnum,
            devid: 2,
            direction: Direction::OUT,
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
      log::debug!(
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

   /// Handle a received unlink package
   fn handle_unlink(&mut self, header: UsbIpHeader, unlink: UsbIpCmdUnlink) {
      match self.unlink(unlink.seqnum) {
         true => (),
         false => {
            log::warn!(
               "received request to remove urb {} that does not exists",
               unlink.seqnum
            );
         }
      }

      self.ack_unlink(header.ep, header.seqnum);
   }

   /// Send an acknowledgement after recieving an unlink package.
   fn ack_unlink(&mut self, ep: u32, seqnum: u32) {
      let response = UsbIpResponse {
         header: UsbIpHeader {
            command: UsbCmd::UnlinkResponse,
            seqnum,
            devid: 2,
            direction: Direction::OUT,
            ep,
         },
         cmd: UsbIpResponseCmd::Unlink(UsbIpRetUnlink { status: 0 }),
         data: vec![],
      };
      log::debug!(
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
