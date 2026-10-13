//! Bulk transfers, where one transfer may span several packets and a short packet, or a
//! zero-length packet after a full one, ends it

use crate::harness::{
    attach, pattern, setup, BULK_IN, BULK_OUT, CMD_ECHO, CMD_FULL_AND_ZLP, CMD_STALL_IN,
    CMD_STALL_OUT, CMD_TWO_PACKETS, CMD_TWO_SHORT, EOVERFLOW, EPIPE, MAX_PACKET_SIZE,
    URB_ZERO_PACKET,
};

/// What a host asks for when it does not know the transfer length in advance
const LARGE: u32 = 4096;

/// A short reply to a short request, each a single packet
#[test]
fn echo() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_ECHO, 1, 2, 3]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), [CMD_ECHO, 1, 2, 3]);
}

/// A transfer longer than one packet completes as a whole
#[test]
fn transfer_of_two_packets() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_TWO_PACKETS]).unwrap();
    assert_eq!(
        client.bulk_in(LARGE).unwrap(),
        pattern(MAX_PACKET_SIZE + 36)
    );
}

/// A transfer of exactly one full packet is ended by a zero-length packet, which must not
/// complete the next transfer
#[test]
fn full_packet_transfer_then_next() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_FULL_AND_ZLP]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), pattern(MAX_PACKET_SIZE));
    client.bulk_out(&[CMD_ECHO, 7]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), [CMD_ECHO, 7]);
}

/// Two transfers queued back to back stay two transfers, each ended by its short packet
#[test]
fn short_transfers_stay_separate() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_TWO_SHORT]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), pattern(10));
    assert_eq!(client.bulk_in(LARGE).unwrap(), pattern(12));
}

/// A URB that fills up before the transfer ends completes, and the rest of the transfer
/// goes to the next URB
#[test]
fn transfer_across_urbs() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_TWO_PACKETS]).unwrap();
    let size = MAX_PACKET_SIZE as u32;
    assert_eq!(client.bulk_in(size).unwrap(), pattern(MAX_PACKET_SIZE));
    assert_eq!(
        client.bulk_in(size).unwrap(),
        pattern(MAX_PACKET_SIZE + 36)[MAX_PACKET_SIZE..]
    );
}

/// A URB waiting before the device has anything to send is answered once it has
#[test]
fn urb_waiting_for_data() {
    let (_device, mut client) = attach();
    let read = client.send(BULK_IN, [0; 8], None, LARGE, 0);
    let write = client.send(BULK_OUT, [0; 8], Some(&[CMD_ECHO, 9]), 0, 0);
    assert_eq!(client.complete(write), Ok(vec![]));
    assert_eq!(client.complete(read), Ok(vec![CMD_ECHO, 9]));
}

/// A packet larger than what is left of the host's buffer is babble
#[test]
fn overflow() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_ECHO, 1, 2, 3]).unwrap();
    assert_eq!(client.bulk_in(2), Err(EOVERFLOW));
}

/// An OUT transfer reaches the device as packets of at most the maximum size, an empty one
/// as a zero-length packet, and one asking for it gets a zero-length packet after a full
/// last packet
#[test]
fn out_packets() {
    let (_device, mut client) = attach();
    let packets =
        |client: &mut crate::harness::Client| client.control_in(setup::vendor_packets()).unwrap();
    client.bulk_out(&pattern(100)).unwrap();
    assert_eq!(packets(&mut client), [64, 36]);
    client.bulk_out(&[]).unwrap();
    assert_eq!(packets(&mut client), [0]);
    let seqnum = client.send(BULK_OUT, [0; 8], Some(&pattern(64)), 0, URB_ZERO_PACKET);
    assert_eq!(client.complete(seqnum), Ok(vec![]));
    assert_eq!(packets(&mut client), [64, 0]);
}

/// A stalled IN endpoint fails its URBs until the host clears the halt
#[test]
fn stalled_in_endpoint() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_STALL_IN]).unwrap();
    assert_eq!(client.bulk_in(LARGE), Err(EPIPE));
    client
        .control_out(setup::clear_halt(0x80 | BULK_IN as u8), &[])
        .unwrap();
    client.bulk_out(&[CMD_ECHO, 4]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), [CMD_ECHO, 4]);
}

/// A stall in the middle of an OUT transfer fails it, with the rest unread
#[test]
fn stall_during_out_transfer() {
    let (_device, mut client) = attach();
    let mut data = pattern(100);
    data[0] = CMD_STALL_OUT;
    assert_eq!(client.bulk_out(&data), Err(EPIPE));
}

/// A stall also fails the URB that is already waiting
#[test]
fn stall_while_urb_waits() {
    let (_device, mut client) = attach();
    let read = client.send(BULK_IN, [0; 8], None, LARGE, 0);
    client.bulk_out(&[CMD_STALL_IN]).unwrap();
    assert_eq!(client.complete(read), Err(EPIPE));
}

/// A stalled OUT endpoint fails its URBs until the host clears the halt
#[test]
fn stalled_out_endpoint() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_STALL_OUT]).unwrap();
    assert_eq!(client.bulk_out(&[CMD_ECHO, 5]), Err(EPIPE));
    client
        .control_out(setup::clear_halt(BULK_OUT as u8), &[])
        .unwrap();
    client.bulk_out(&[CMD_ECHO, 5]).unwrap();
    assert_eq!(client.bulk_in(LARGE).unwrap(), [CMD_ECHO, 5]);
}
