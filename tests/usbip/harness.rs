//! The test device, and the USB/IP client the tests drive it with

use std::{
    collections::{HashMap, VecDeque},
    convert::TryInto,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use usb_device::{
    bus::{InterfaceNumber, UsbBus, UsbBusAllocator},
    class::{ControlIn, ControlOut, UsbClass},
    control::RequestType,
    descriptor::DescriptorWriter,
    device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid},
    endpoint::{EndpointAddress, EndpointIn, EndpointOut, EndpointType},
    LangID,
};
use usbip_device::UsbIpBus;

/// Bulk endpoint numbers of the test device, fixed so tests need no descriptor parsing
pub const BULK_OUT: u32 = 1;
pub const BULK_IN: u32 = 1;
/// The packet size of every endpoint, endpoint 0 included
pub const MAX_PACKET_SIZE: usize = 64;

/// Vendor requests the test device answers
pub const REQ_READ: u8 = 1;
pub const REQ_ACCEPT: u8 = 2;
pub const REQ_REJECT: u8 = 3;
pub const REQ_STORE: u8 = 4;
pub const REQ_LOAD: u8 = 5;
pub const REQ_PACKETS: u8 = 6;

/// Commands sent as the first byte of a bulk OUT transfer
pub const CMD_NOTHING: u8 = b'n';
pub const CMD_ECHO: u8 = b'e';
pub const CMD_TWO_SHORT: u8 = b't';
pub const CMD_FULL_AND_ZLP: u8 = b'f';
pub const CMD_TWO_PACKETS: u8 = b'm';
pub const CMD_STALL_IN: u8 = b's';
pub const CMD_STALL_OUT: u8 = b'S';

/// Linux error codes, as the host sees them in a URB's status
pub const EPIPE: i32 = -32;
pub const EOVERFLOW: i32 = -75;
pub const ECONNRESET: i32 = -104;

/// URB transfer flag asking for a zero-length packet after a full last one
pub const URB_ZERO_PACKET: u32 = 0x40;

/// The bytes vendor reads and bulk replies are cut from
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| i as u8).collect()
}

struct Probe<'a, B: UsbBus> {
    interface: InterfaceNumber,
    ep_out: EndpointOut<'a, B>,
    ep_in: EndpointIn<'a, B>,
    outbox: VecDeque<Vec<u8>>,
    /// What the last control OUT data stage carried
    stored: Vec<u8>,
    /// The length of every bulk OUT packet read
    packets: Vec<u8>,
}

impl<'a, B: UsbBus> Probe<'a, B> {
    fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        let ea_out = EndpointAddress::from(BULK_OUT as u8);
        let ea_in = EndpointAddress::from(0x80 | BULK_IN as u8);
        Self {
            interface: alloc.interface(),
            ep_out: alloc
                .alloc(Some(ea_out), EndpointType::Bulk, MAX_PACKET_SIZE as u16, 0)
                .expect("bulk OUT endpoint"),
            ep_in: alloc
                .alloc(Some(ea_in), EndpointType::Bulk, MAX_PACKET_SIZE as u16, 0)
                .expect("bulk IN endpoint"),
            outbox: VecDeque::new(),
            stored: Vec::new(),
            packets: Vec::new(),
        }
    }

    /// Writes queued packets in order, as a class driver would: until the bus is not ready
    /// for the next one, and then again once it reports the last one sent
    fn flush(&mut self) {
        while let Some(packet) = self.outbox.front() {
            match self.ep_in.write(packet) {
                Ok(_) => {
                    self.outbox.pop_front();
                }
                Err(_) => break,
            }
        }
    }
}

impl<B: UsbBus> UsbClass<B> for Probe<'_, B> {
    fn get_configuration_descriptors(
        &self,
        writer: &mut DescriptorWriter,
    ) -> usb_device::Result<()> {
        writer.interface(self.interface, 0xff, 0, 0)?;
        writer.endpoint(&self.ep_out)?;
        writer.endpoint(&self.ep_in)
    }

    fn reset(&mut self) {
        self.outbox.clear();
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = *xfer.request();
        if req.request_type != RequestType::Vendor {
            return;
        }
        match req.request {
            // wValue selects the reply length
            REQ_READ => xfer.accept_with(&pattern(req.value as usize)).ok(),
            REQ_LOAD => xfer.accept_with(&self.stored).ok(),
            REQ_PACKETS => xfer.accept_with(&std::mem::take(&mut self.packets)).ok(),
            _ => None,
        };
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = *xfer.request();
        if req.request_type != RequestType::Vendor {
            return;
        }
        match req.request {
            REQ_ACCEPT => xfer.accept().ok(),
            REQ_REJECT => xfer.reject().ok(),
            REQ_STORE => {
                self.stored = xfer.data().to_vec();
                xfer.accept().ok()
            }
            _ => None,
        };
    }

    fn endpoint_out(&mut self, addr: EndpointAddress) {
        if addr != self.ep_out.address() {
            return;
        }
        let mut buf = [0; MAX_PACKET_SIZE];
        let Ok(len) = self.ep_out.read(&mut buf) else {
            return;
        };
        self.packets.push(len as u8);
        match buf[..len].first() {
            Some(&CMD_ECHO) => self.outbox.push_back(buf[..len].to_vec()),
            // Two transfers back to back, each a single short packet
            Some(&CMD_TWO_SHORT) => {
                self.outbox.push_back(pattern(10));
                self.outbox.push_back(pattern(12));
            }
            // One transfer of exactly one full packet, ended by a zero-length packet
            Some(&CMD_FULL_AND_ZLP) => {
                self.outbox.push_back(pattern(MAX_PACKET_SIZE));
                self.outbox.push_back(Vec::new());
            }
            // One transfer spanning a full packet and a short one
            Some(&CMD_TWO_PACKETS) => {
                let reply = pattern(MAX_PACKET_SIZE + 36);
                self.outbox.push_back(reply[..MAX_PACKET_SIZE].to_vec());
                self.outbox.push_back(reply[MAX_PACKET_SIZE..].to_vec());
            }
            Some(&CMD_STALL_IN) => self.ep_in.stall(),
            Some(&CMD_STALL_OUT) => self.ep_out.stall(),
            _ => {}
        }
        self.flush();
    }

    fn endpoint_in_complete(&mut self, addr: EndpointAddress) {
        if addr == self.ep_in.address() {
            self.flush();
        }
    }
}

/// The bus binds a fixed port, so devices run one at a time
static PORT: Mutex<()> = Mutex::new(());

/// A running test device; dropping it stops the device and frees the port
pub struct Device {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _port: MutexGuard<'static, ()>,
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// Starts a test device and attaches a client to it
pub fn attach() -> (Device, Client) {
    let port = PORT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Any other USB/IP server on the port would answer in place of the test device
    if let Err(err) = TcpListener::bind(("127.0.0.1", 3240)) {
        panic!("port 3240 is taken, stop other USB/IP servers: {}", err);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread = thread::spawn({
        let stop = stop.clone();
        move || {
            let alloc = UsbBusAllocator::new(UsbIpBus::new());
            let mut probe = Probe::new(&alloc);
            let mut device = UsbDeviceBuilder::new(&alloc, UsbVidPid(0x1209, 0x0001))
                .strings(&[StringDescriptors::new(LangID::EN).product("usbip-device test")])
                .expect("string descriptors")
                .max_packet_size_0(MAX_PACKET_SIZE as u8)
                .expect("endpoint 0 packet size")
                .build();
            while !stop.load(Ordering::Relaxed) {
                device.poll(&mut [&mut probe]);
                thread::sleep(Duration::from_micros(200));
            }
        }
    });
    let device = Device {
        stop,
        thread: Some(thread),
        _port: port,
    };
    (device, Client::connect())
}

/// What the device sent back for one request
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// USBIP_RET_SUBMIT: a URB completed with a status and, for IN, its data
    Submit {
        seqnum: u32,
        status: i32,
        data: Vec<u8>,
    },
    /// USBIP_RET_UNLINK
    Unlink { seqnum: u32, status: i32 },
}

/// A USB/IP client speaking to the test device over TCP
pub struct Client {
    stream: TcpStream,
    seqnum: u32,
    /// Whether each URB in flight is IN, since only IN replies carry data and the reply
    /// header need not say
    dir_in: HashMap<u32, bool>,
}

/// How long a test waits for any single reply
pub const URB_TIMEOUT: Duration = Duration::from_secs(2);

impl Client {
    /// Connects and imports the device, as `usbip attach` does
    pub fn connect() -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", 3240)) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(err) => panic!("device did not start listening: {}", err),
            }
        };
        stream.set_read_timeout(Some(URB_TIMEOUT)).unwrap();
        // Requests go out the moment a test sends them, so several can be in flight
        stream.set_nodelay(true).unwrap();

        // OP_REQ_IMPORT for bus id 1-1
        let mut request = vec![0x01, 0x11, 0x80, 0x03, 0, 0, 0, 0];
        let mut bus_id = [0; 32];
        bus_id[..3].copy_from_slice(b"1-1");
        request.extend_from_slice(&bus_id);
        stream.write_all(&request).unwrap();

        let mut reply = [0; 8 + 312];
        stream.read_exact(&mut reply).expect("OP_REP_IMPORT");
        assert_eq!(&reply[2..8], &[0x00, 0x03, 0, 0, 0, 0], "import refused");
        Self {
            stream,
            seqnum: 0,
            dir_in: HashMap::new(),
        }
    }

    /// Submits a URB without waiting for it, returning its sequence number. An OUT URB
    /// (`out` given) is as long as its data, an IN URB asks for `len` bytes.
    pub fn send(
        &mut self,
        ep: u32,
        setup: [u8; 8],
        out: Option<&[u8]>,
        len: u32,
        flags: u32,
    ) -> u32 {
        self.seqnum += 1;
        let direction: u32 = if out.is_some() { 0 } else { 1 };
        let length = out.map_or(len, |data| data.len() as u32);
        let mut cmd = Vec::with_capacity(48);
        for word in [
            1,
            self.seqnum,
            0x0001_0002,
            direction,
            ep,
            flags,
            length,
            0,
            0,
            0,
        ] {
            cmd.extend_from_slice(&u32::to_be_bytes(word));
        }
        cmd.extend_from_slice(&setup);
        cmd.extend_from_slice(out.unwrap_or_default());
        self.stream.write_all(&cmd).unwrap();
        self.dir_in.insert(self.seqnum, direction == 1);
        self.seqnum
    }

    /// Asks the device to drop a URB, returning the unlink's own sequence number
    pub fn unlink(&mut self, seqnum: u32) -> u32 {
        self.seqnum += 1;
        let mut cmd = Vec::with_capacity(48);
        for word in [2, self.seqnum, 0x0001_0002, 0, 0, seqnum] {
            cmd.extend_from_slice(&u32::to_be_bytes(word));
        }
        cmd.resize(48, 0);
        self.stream.write_all(&cmd).unwrap();
        self.seqnum
    }

    /// Waits for the next reply, failing the test if none comes
    pub fn reply(&mut self) -> Reply {
        let mut header = [0; 48];
        if let Err(err) = self.stream.read_exact(&mut header) {
            panic!("no reply: {}", err);
        }
        let word = |i: usize| u32::from_be_bytes(header[i * 4..i * 4 + 4].try_into().unwrap());
        let seqnum = word(1);
        let status = word(5) as i32;
        match word(0) {
            3 => {
                let mut data = Vec::new();
                if self
                    .dir_in
                    .remove(&seqnum)
                    .expect("reply to a URB in flight")
                {
                    data.resize(word(6) as usize, 0);
                    self.stream.read_exact(&mut data).unwrap();
                }
                Reply::Submit {
                    seqnum,
                    status,
                    data,
                }
            }
            4 => Reply::Unlink { seqnum, status },
            command => panic!("unexpected reply command {}", command),
        }
    }

    /// Waits for the reply to `seqnum`, which must be the next one, and returns its data or
    /// the error status it completed with
    pub fn complete(&mut self, seqnum: u32) -> Result<Vec<u8>, i32> {
        match self.reply() {
            Reply::Submit {
                seqnum: got,
                status,
                data,
            } => {
                assert_eq!(got, seqnum, "reply for another URB");
                match status {
                    0 => Ok(data),
                    status => Err(status),
                }
            }
            reply => panic!("expected the reply to URB {}, got {:?}", seqnum, reply),
        }
    }

    pub fn control_in(&mut self, setup: [u8; 8]) -> Result<Vec<u8>, i32> {
        let len = u16::from_le_bytes([setup[6], setup[7]]) as u32;
        let seqnum = self.send(0, setup, None, len, 0);
        self.complete(seqnum)
    }

    pub fn control_out(&mut self, setup: [u8; 8], data: &[u8]) -> Result<(), i32> {
        let seqnum = self.send(0, setup, Some(data), 0, 0);
        self.complete(seqnum).map(drop)
    }

    pub fn bulk_out(&mut self, data: &[u8]) -> Result<(), i32> {
        let seqnum = self.send(BULK_OUT, [0; 8], Some(data), 0, 0);
        self.complete(seqnum).map(drop)
    }

    pub fn bulk_in(&mut self, len: u32) -> Result<Vec<u8>, i32> {
        let seqnum = self.send(BULK_IN, [0; 8], None, len, 0);
        self.complete(seqnum)
    }
}

/// Setup packets used by the tests
pub mod setup {
    pub fn get_device_descriptor(len: u16) -> [u8; 8] {
        let [lo, hi] = len.to_le_bytes();
        [0x80, 0x06, 0x00, 0x01, 0, 0, lo, hi]
    }

    pub fn set_configuration(value: u8) -> [u8; 8] {
        [0x00, 0x09, value, 0, 0, 0, 0, 0]
    }

    /// CLEAR_FEATURE(ENDPOINT_HALT) for the endpoint with address `ep`
    pub fn clear_halt(ep: u8) -> [u8; 8] {
        [0x02, 0x01, 0, 0, ep, 0, 0, 0]
    }

    /// Vendor read of `reply` bytes, asking for up to `len`
    pub fn vendor_read(reply: u16, len: u16) -> [u8; 8] {
        let [vlo, vhi] = reply.to_le_bytes();
        let [lo, hi] = len.to_le_bytes();
        [0xc0, super::REQ_READ, vlo, vhi, 0, 0, lo, hi]
    }

    /// A vendor IN request the device does not know, and so stalls
    pub fn vendor_unknown_in() -> [u8; 8] {
        [0xc0, 0xff, 0, 0, 0, 0, 8, 0]
    }

    pub fn vendor_accept() -> [u8; 8] {
        [0x40, super::REQ_ACCEPT, 0, 0, 0, 0, 0, 0]
    }

    pub fn vendor_reject() -> [u8; 8] {
        [0x40, super::REQ_REJECT, 0, 0, 0, 0, 0, 0]
    }

    /// Vendor write of `len` bytes, which the device keeps
    pub fn vendor_store(len: u16) -> [u8; 8] {
        let [lo, hi] = len.to_le_bytes();
        [0x40, super::REQ_STORE, 0, 0, 0, 0, lo, hi]
    }

    /// Vendor read of what the last store wrote
    pub fn vendor_load(len: u16) -> [u8; 8] {
        let [lo, hi] = len.to_le_bytes();
        [0xc0, super::REQ_LOAD, 0, 0, 0, 0, lo, hi]
    }

    /// Vendor read of the lengths of the bulk OUT packets the device read since last asked
    pub fn vendor_packets() -> [u8; 8] {
        [0xc0, super::REQ_PACKETS, 0, 0, 0, 0, 64, 0]
    }
}
