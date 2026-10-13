//! Endpoint state, kept per URB rather than per packet.
//!
//! USB/IP hands the device whole transfers and expects exactly one reply to each, while
//! `usb-device` moves single packets. The pipes here translate between the two with the
//! completion rules of USB itself: an IN transfer ends with a short packet or once the host's
//! buffer is full, an OUT transfer once the device has read all of its packets, and a control
//! transfer with its status stage or a stall.

use std::collections::VecDeque;
use usb_device::{Result as UsbResult, UsbError};

/// Linux error codes, which the host expects in the status field of a reply
pub(crate) const EPIPE: i32 = -32;
pub(crate) const EOVERFLOW: i32 = -75;
pub(crate) const ECONNRESET: i32 = -104;

/// A URB the host has submitted and is waiting on
#[derive(Debug)]
pub(crate) struct Urb {
    pub seqnum: u32,
    pub ep: u32,
    pub dir_in: bool,
    pub setup: [u8; 8],
    /// The size of the host's buffer
    pub length: usize,
    /// The data to send for OUT, the data gathered so far for IN
    pub data: Vec<u8>,
    /// How much OUT data the device has read
    pub consumed: usize,
}

impl Urb {
    fn finish(self, status: i32) -> Completion {
        let (actual_length, data) = if self.dir_in {
            (self.data.len(), self.data)
        } else {
            (self.consumed, Vec::new())
        };
        Completion {
            seqnum: self.seqnum,
            ep: self.ep,
            dir_in: self.dir_in,
            status,
            actual_length,
            data,
        }
    }

    /// Moves an IN packet into the URB and returns the status it completes with, if the
    /// packet ends the transfer
    fn fill(&mut self, packet: &[u8], max_packet_size: usize) -> Option<i32> {
        let room = self.length - self.data.len();
        if packet.len() > room {
            // More data than the host asked for, which a host controller reports as babble
            self.data.extend_from_slice(&packet[..room]);
            return Some(EOVERFLOW);
        }
        self.data.extend_from_slice(packet);
        if packet.len() < max_packet_size || self.data.len() == self.length {
            Some(0)
        } else {
            None
        }
    }
}

/// The reply to a URB
#[derive(Debug)]
pub(crate) struct Completion {
    pub seqnum: u32,
    pub ep: u32,
    pub dir_in: bool,
    pub status: i32,
    pub actual_length: usize,
    pub data: Vec<u8>,
}

/// Copies the next packet into `buf` and takes it off the queue, unless it does not fit, in
/// which case it stays for a read with a larger buffer
fn read_packet<T>(
    queue: &mut VecDeque<T>,
    data: impl Fn(&T) -> &[u8],
    buf: &mut [u8],
) -> UsbResult<T> {
    let packet = data(queue.front().ok_or(UsbError::WouldBlock)?);
    buf.get_mut(..packet.len())
        .ok_or(UsbError::BufferOverflow)?
        .copy_from_slice(packet);
    Ok(queue.pop_front().expect("a packet"))
}

/// A bulk or interrupt IN endpoint
#[derive(Debug)]
pub(crate) struct InPipe {
    max_packet_size: usize,
    stalled: bool,
    urbs: VecDeque<Urb>,
    /// The endpoint's packet buffer, which holds one packet until a URB takes it, so the
    /// device cannot run ahead of the host
    buffer: Option<Vec<u8>>,
    /// Whether a packet left the buffer since the last poll
    sent: bool,
}

impl InPipe {
    pub fn new(max_packet_size: u16) -> Self {
        Self {
            max_packet_size: max_packet_size as usize,
            stalled: false,
            urbs: VecDeque::new(),
            buffer: None,
            sent: false,
        }
    }

    pub fn submit(&mut self, urb: Urb) -> Option<Completion> {
        if self.stalled {
            return Some(urb.finish(EPIPE));
        }
        self.urbs.push_back(urb);
        self.drain()
    }

    pub fn write(&mut self, packet: &[u8]) -> UsbResult<Option<Completion>> {
        if packet.len() > self.max_packet_size {
            return Err(UsbError::BufferOverflow);
        }
        if self.buffer.is_some() {
            return Err(UsbError::WouldBlock);
        }
        self.buffer = Some(packet.to_vec());
        Ok(self.drain())
    }

    fn drain(&mut self) -> Option<Completion> {
        let urb = self.urbs.front_mut()?;
        let packet = self.buffer.take()?;
        self.sent = true;
        let status = urb.fill(&packet, self.max_packet_size)?;
        self.urbs.pop_front().map(|urb| urb.finish(status))
    }

    pub fn take_sent(&mut self) -> bool {
        std::mem::take(&mut self.sent)
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    /// A stalled endpoint fails every URB until the host clears the halt
    pub fn set_stalled(&mut self, stalled: bool) -> Vec<Completion> {
        self.stalled = stalled;
        if !stalled {
            return Vec::new();
        }
        self.buffer = None;
        self.urbs.drain(..).map(|urb| urb.finish(EPIPE)).collect()
    }

    pub fn unlink(&mut self, seqnum: u32) -> bool {
        let before = self.urbs.len();
        self.urbs.retain(|urb| urb.seqnum != seqnum);
        before != self.urbs.len()
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.max_packet_size as u16);
    }
}

/// A bulk or interrupt OUT endpoint
#[derive(Debug)]
pub(crate) struct OutPipe {
    max_packet_size: usize,
    stalled: bool,
    /// Every URB here has at least one packet left to read
    urbs: VecDeque<(Urb, VecDeque<Vec<u8>>)>,
}

impl OutPipe {
    pub fn new(max_packet_size: u16) -> Self {
        Self {
            max_packet_size: max_packet_size as usize,
            stalled: false,
            urbs: VecDeque::new(),
        }
    }

    /// `zero_packet` is the host asking for a full last packet to be followed by a
    /// zero-length one
    pub fn submit(&mut self, urb: Urb, zero_packet: bool) -> Option<Completion> {
        if self.stalled {
            return Some(urb.finish(EPIPE));
        }
        let mut packets: VecDeque<_> = urb
            .data
            .chunks(self.max_packet_size)
            .map(<[u8]>::to_vec)
            .collect();
        let full_last_packet = packets
            .back()
            .is_some_and(|packet| packet.len() == self.max_packet_size);
        // An empty transfer is still one packet on the wire
        if packets.is_empty() || (zero_packet && full_last_packet) {
            packets.push_back(Vec::new());
        }
        self.urbs.push_back((urb, packets));
        None
    }

    pub fn has_packet(&self) -> bool {
        !self.urbs.is_empty()
    }

    /// Reads the next packet; the URB it belonged to completes once its last packet is read
    pub fn read(&mut self, buf: &mut [u8]) -> (UsbResult<usize>, Option<Completion>) {
        let Some((urb, packets)) = self.urbs.front_mut() else {
            return (Err(UsbError::WouldBlock), None);
        };
        let packet = match read_packet(packets, |packet| packet, buf) {
            Ok(packet) => packet,
            Err(err) => return (Err(err), None),
        };
        urb.consumed += packet.len();
        let completion = if packets.is_empty() {
            self.urbs.pop_front().map(|(urb, _)| urb.finish(0))
        } else {
            None
        };
        (Ok(packet.len()), completion)
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    pub fn set_stalled(&mut self, stalled: bool) -> Vec<Completion> {
        self.stalled = stalled;
        if !stalled {
            return Vec::new();
        }
        self.urbs
            .drain(..)
            .map(|(urb, _)| urb.finish(EPIPE))
            .collect()
    }

    pub fn unlink(&mut self, seqnum: u32) -> bool {
        let before = self.urbs.len();
        self.urbs.retain(|(urb, _)| urb.seqnum != seqnum);
        before != self.urbs.len()
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.max_packet_size as u16);
    }
}

/// The two kinds of packet the device reads on endpoint 0, which `poll` reports apart
pub(crate) enum PacketKind {
    Setup,
    Data,
}

/// A packet for the device to read on endpoint 0
#[derive(Debug)]
struct ControlPacket {
    setup: bool,
    data: Vec<u8>,
}

/// Endpoint 0, which runs one control transfer at a time.
///
/// A USB/IP control URB is a whole transfer: its setup, its data and an implied status
/// stage. The URB is answered when the device ends the transfer, with its status stage for
/// OUT, with the last data packet for IN, or with a stall. Until the device has read the next
/// setup packet, nothing it writes can belong to the next transfer. Such writes are dropped
/// rather than answering the wrong URB: the zero-length packet `usb-device` sends after a
/// reply that fills the host's buffer exactly, or the rest of a transfer the host unlinked.
#[derive(Debug)]
pub(crate) struct ControlPipe {
    max_packet_size: usize,
    stalled: bool,
    /// URBs waiting for the current transfer to finish
    queue: VecDeque<Urb>,
    /// The transfer in progress, from the moment its setup packet is offered to the device
    current: Option<Urb>,
    /// Whether the device has read the setup packet of the current transfer
    started: bool,
    /// Packets of the current transfer for the device to read: its setup, then OUT data
    packets: VecDeque<ControlPacket>,
    /// Whether the device wrote a packet to the current transfer since the last poll
    sent: bool,
}

impl ControlPipe {
    pub fn new(max_packet_size: u16) -> Self {
        Self {
            max_packet_size: max_packet_size as usize,
            stalled: false,
            queue: VecDeque::new(),
            current: None,
            started: false,
            packets: VecDeque::new(),
            sent: false,
        }
    }

    pub fn submit(&mut self, urb: Urb) {
        self.queue.push_back(urb);
        if self.current.is_none() {
            self.start_next();
        }
    }

    fn start_next(&mut self) {
        let Some(urb) = self.queue.pop_front() else {
            return;
        };
        self.packets.push_back(ControlPacket {
            setup: true,
            data: urb.setup.to_vec(),
        });
        if !urb.dir_in {
            for chunk in urb.data.chunks(self.max_packet_size) {
                self.packets.push_back(ControlPacket {
                    setup: false,
                    data: chunk.to_vec(),
                });
            }
        }
        self.current = Some(urb);
        self.started = false;
    }

    /// What the device would read next, if anything
    pub fn next_packet(&self) -> Option<PacketKind> {
        self.packets.front().map(|packet| {
            if packet.setup {
                PacketKind::Setup
            } else {
                PacketKind::Data
            }
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> UsbResult<usize> {
        let packet = read_packet(&mut self.packets, |packet| &packet.data, buf)?;
        if packet.setup {
            // A setup packet clears a stall on endpoint 0
            self.started = true;
            self.stalled = false;
        }
        Ok(packet.data.len())
    }

    pub fn write(&mut self, packet: &[u8]) -> UsbResult<Option<Completion>> {
        if packet.len() > self.max_packet_size {
            return Err(UsbError::BufferOverflow);
        }
        let urb = match self.current.as_mut() {
            Some(urb) if self.started => urb,
            // Reported as never sent: before 0.3.2, `usb-device` handles an IN completion
            // ahead of the next setup, and one it does not expect stalls that transfer
            _ => return Ok(None),
        };
        self.sent = true;
        let status = if urb.dir_in {
            urb.fill(packet, self.max_packet_size)
        } else {
            // The status stage of an OUT transfer
            Some(0)
        };
        Ok(status.map(|status| self.finish(status)))
    }

    fn finish(&mut self, status: i32) -> Completion {
        let mut urb = self.current.take().expect("a transfer in progress");
        self.started = false;
        self.packets.clear();
        if status == 0 && !urb.dir_in {
            urb.consumed = urb.data.len();
        }
        self.start_next();
        urb.finish(status)
    }

    pub fn take_sent(&mut self) -> bool {
        std::mem::take(&mut self.sent)
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    /// A stall ends the current transfer with an error; the next setup clears it
    pub fn set_stalled(&mut self, stalled: bool) -> Option<Completion> {
        self.stalled = stalled;
        if stalled && self.started {
            Some(self.finish(EPIPE))
        } else {
            None
        }
    }

    pub fn unlink(&mut self, seqnum: u32) -> bool {
        if self.current.as_ref().map(|urb| urb.seqnum) == Some(seqnum) {
            self.current = None;
            self.started = false;
            self.packets.clear();
            self.start_next();
            return true;
        }
        let before = self.queue.len();
        self.queue.retain(|urb| urb.seqnum != seqnum);
        before != self.queue.len()
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.max_packet_size as u16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_urb(data: &[u8]) -> Urb {
        Urb {
            seqnum: 1,
            ep: 1,
            dir_in: false,
            setup: [0; 8],
            length: data.len(),
            data: data.to_vec(),
            consumed: 0,
        }
    }

    /// A class reading with too small a buffer gets an error, not a cut packet, and can
    /// read the packet again with a buffer that fits
    #[test]
    fn out_packet_too_large_for_buffer_stays() {
        let mut pipe = OutPipe::new(64);
        assert!(pipe.submit(out_urb(&[1, 2, 3]), false).is_none());
        let (result, completion) = pipe.read(&mut [0; 2]);
        assert!(matches!(result, Err(UsbError::BufferOverflow)));
        assert!(completion.is_none());
        let mut buf = [0; 64];
        let (result, completion) = pipe.read(&mut buf);
        assert_eq!(result.ok(), Some(3));
        assert_eq!(buf[..3], [1, 2, 3]);
        let completion = completion.expect("the URB completes with its last packet");
        assert_eq!((completion.status, completion.actual_length), (0, 3));
    }
}
