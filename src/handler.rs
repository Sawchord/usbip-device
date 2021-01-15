use crate::UsbIpBus;
use std::{
   io::Read,
   net::{TcpListener, TcpStream},
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
      let mut buf = [0; 4096];

      loop {
         let bytes_read = stream.read(&mut buf).unwrap();
         log::debug!("read {} bytes from socket", bytes_read);
      }
   }
}
