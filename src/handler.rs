use crate::{
    cmd::{Direction, TransferFlags, UsbCmd, UsbIpHeader},
    endpoint::{Completion, Urb, ECONNRESET},
    op::{OpDeviceDescriptor, OpInterfaceDescriptor, OpRequest, OpResponse, OpResponseCommand},
    request::{UsbIpCmdSubmit, UsbIpCmdUnlink, UsbIpRequest, UsbIpRequestCmd},
    response::{UsbIpResponse, UsbIpResponseCmd, UsbIpRetSubmit, UsbIpRetUnlink},
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
                    // Replies often go out back to back, which Nagle's algorithm would hold
                    // until the host acknowledges the first
                    connection.set_nodelay(true).ok();
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
                            Err(err) if is_disconnect(&err) => {
                                self.disconnect();
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
                            Err(err) if is_disconnect(&err) => {
                                self.disconnect();
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

    /// Answers finished URBs
    pub fn send(&mut self, completions: impl IntoIterator<Item = Completion>) {
        for completion in completions {
            let response = UsbIpResponse {
                header: UsbIpHeader {
                    command: UsbCmd::Response,
                    seqnum: completion.seqnum,
                    devid: 2,
                    direction: if completion.dir_in {
                        Direction::IN
                    } else {
                        Direction::OUT
                    },
                    ep: completion.ep,
                },
                cmd: UsbIpResponseCmd::Cmd(UsbIpRetSubmit {
                    status: completion.status,
                    actual_length: completion.actual_length as i32,
                    start_frame: 0,
                    number_of_packets: 0,
                    error_count: 0,
                }),
                data: completion.data,
            };
            log::debug!("{:?}", response);
            self.reply(&response.to_vec().unwrap());
        }
    }

    /// Writes a reply to the host, if it is still there
    fn reply(&mut self, bytes: &[u8]) {
        // The host is gone, and with it everything it was waiting on
        let Some(connection) = self.handler.connection.as_mut() else {
            return;
        };
        // Reads leave the socket non-blocking, but a reply must go out whole
        let result = connection
            .set_nonblocking(false)
            .and_then(|()| connection.write_all(bytes));
        if let Err(err) = result {
            log::info!("connection lost: {}", err);
            self.disconnect();
        }
    }

    /// Returns to the initial state, ready for the next host
    fn disconnect(&mut self) {
        self.handler.connection = None;
        self.reset = true;
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

                self.reply(&list_response.to_vec().unwrap());
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

                self.reply(&list_response.to_vec().unwrap());
            }
        }
    }

    fn handle_usbip_pkg(&mut self, request: UsbIpRequest) {
        log::debug!("{:?}", request);

        match request.cmd {
            UsbIpRequestCmd::Unlink(unlink) => self.handle_unlink(request.header, unlink),
            UsbIpRequestCmd::Cmd(cmd) => self.handle_cmd(request.header, cmd, request.data),
        }
    }

    /// Hands a submitted URB to its endpoint
    fn handle_cmd(&mut self, header: UsbIpHeader, cmd: UsbIpCmdSubmit, data: Vec<u8>) {
        let index = header.ep as usize;
        let dir_in = header.direction == Direction::IN;
        let urb = Urb {
            seqnum: header.seqnum,
            ep: header.ep,
            dir_in,
            setup: cmd.setup,
            length: cmd.transfer_buffer_length.max(0) as usize,
            data,
            consumed: 0,
        };
        let zero_packet = cmd.transfer_flags.contains(TransferFlags::ZERO_PACKET);

        let completion = match (index, dir_in) {
            // Control URBs are answered by the device, never on arrival
            (0, _) => self.control().map(|control| {
                control.submit(urb);
                None
            }),
            (_, true) => self.pipe_in(index).map(|pipe| pipe.submit(urb)),
            (_, false) => self
                .pipe_out(index)
                .map(|pipe| pipe.submit(urb, zero_packet)),
        };
        match completion {
            Ok(completion) => self.send(completion),
            Err(err) => log::warn!(
                "received URB for unimplemented endpoint {}: {:?}",
                index,
                err
            ),
        }
    }

    /// Handle a received unlink package
    fn handle_unlink(&mut self, header: UsbIpHeader, unlink: UsbIpCmdUnlink) {
        // A URB that was already answered is no error: the host simply raced the reply
        let status = if self.unlink(unlink.seqnum) {
            ECONNRESET
        } else {
            0
        };

        self.ack_unlink(header.ep, header.seqnum, status);
    }

    /// Send an acknowledgement after recieving an unlink package.
    fn ack_unlink(&mut self, ep: u32, seqnum: u32, status: i32) {
        let response = UsbIpResponse {
            header: UsbIpHeader {
                command: UsbCmd::UnlinkResponse,
                seqnum,
                devid: 2,
                direction: Direction::OUT,
                ep,
            },
            cmd: UsbIpResponseCmd::Unlink(UsbIpRetUnlink { status }),
            data: vec![],
        };
        log::debug!("{:?}", response);

        self.reply(&response.to_vec().unwrap());
    }
}

/// Whether a socket error means the host went away, rather than a bug
fn is_disconnect(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::NotConnected
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
    )
}
