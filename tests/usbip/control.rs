//! Control transfers on endpoint 0, each of which is a single URB carrying the setup
//! packet, the data stage and an implied status stage

use crate::harness::{attach, pattern, setup, CMD_NOTHING, EPIPE, MAX_PACKET_SIZE};

/// A standard request the device answers in a single packet
#[test]
fn control_in() {
    let (_device, mut client) = attach();
    let descriptor = client.control_in(setup::get_device_descriptor(18)).unwrap();
    assert_eq!(descriptor.len(), 18);
    assert_eq!(descriptor[1], 0x01, "device descriptor type");
}

/// The host may ask for less than the device has
#[test]
fn control_in_shorter_than_reply() {
    let (_device, mut client) = attach();
    let descriptor = client.control_in(setup::get_device_descriptor(8)).unwrap();
    assert_eq!(descriptor.len(), 8);
    assert_eq!(descriptor[1], 0x01, "device descriptor type");
}

/// A control OUT is answered once the device acknowledges it in the status stage, and
/// that acknowledgement must not answer the next control IN instead
#[test]
fn control_in_after_control_out() {
    let (_device, mut client) = attach();
    client
        .control_out(setup::set_configuration(1), &[])
        .unwrap();
    let descriptor = client.control_in(setup::get_device_descriptor(18)).unwrap();
    assert_eq!(descriptor.len(), 18);
    assert_eq!(descriptor[1], 0x01, "device descriptor type");
}

/// Several control OUTs in a row each end with their own status stage
#[test]
fn control_in_after_control_outs() {
    let (_device, mut client) = attach();
    for _ in 0..3 {
        client.control_out(setup::vendor_accept(), &[]).unwrap();
    }
    let reply = client.control_in(setup::vendor_read(5, 64)).unwrap();
    assert_eq!(reply, pattern(5));
}

/// A reply of exactly one full packet that is shorter than requested ends with a
/// zero-length packet, which belongs to the data stage
#[test]
fn full_packet_control_in() {
    let (_device, mut client) = attach();
    let reply = client
        .control_in(setup::vendor_read(MAX_PACKET_SIZE as u16, 255))
        .unwrap();
    assert_eq!(reply, pattern(MAX_PACKET_SIZE));
}

/// A reply that fills the host's buffer exactly is complete without a zero-length packet,
/// and one sent anyway must not answer the next request
#[test]
fn exact_full_packet_control_in() {
    let (_device, mut client) = attach();
    let size = MAX_PACKET_SIZE as u16;
    let reply = client.control_in(setup::vendor_read(size, size)).unwrap();
    assert_eq!(reply, pattern(MAX_PACKET_SIZE));
    let reply = client.control_in(setup::vendor_read(5, 64)).unwrap();
    assert_eq!(reply, pattern(5));
}

/// A reply longer than one packet arrives whole in a single URB
#[test]
fn control_in_of_two_packets() {
    let (_device, mut client) = attach();
    let reply = client.control_in(setup::vendor_read(100, 255)).unwrap();
    assert_eq!(reply, pattern(100));
}

/// The data stage zero-length packet must survive bulk OUT traffic before it
#[test]
fn full_packet_control_in_after_bulk_out() {
    let (_device, mut client) = attach();
    client.bulk_out(&[CMD_NOTHING]).unwrap();
    let reply = client
        .control_in(setup::vendor_read(MAX_PACKET_SIZE as u16, 255))
        .unwrap();
    assert_eq!(reply, pattern(MAX_PACKET_SIZE));
}

/// The data stage zero-length packet must also survive a control OUT the device rejects,
/// which leaves no status stage behind
#[test]
fn full_packet_control_in_after_rejected_control_out() {
    let (_device, mut client) = attach();
    client.control_out(setup::vendor_reject(), &[]).ok();
    let reply = client
        .control_in(setup::vendor_read(MAX_PACKET_SIZE as u16, 255))
        .unwrap();
    assert_eq!(reply, pattern(MAX_PACKET_SIZE));
}

/// A data stage spanning two packets reaches the device whole
#[test]
fn control_out_with_data() {
    let (_device, mut client) = attach();
    client
        .control_out(setup::vendor_store(100), &pattern(100))
        .unwrap();
    assert_eq!(
        client.control_in(setup::vendor_load(255)).unwrap(),
        pattern(100)
    );
}

/// A request the device rejects completes with a stall, and the next one works again
#[test]
fn rejected_control_in_is_stalled() {
    let (_device, mut client) = attach();
    assert_eq!(client.control_in(setup::vendor_unknown_in()), Err(EPIPE));
    assert_eq!(
        client.control_in(setup::vendor_read(5, 64)).unwrap(),
        pattern(5)
    );
}

/// A control OUT the device rejects completes with a stall too
#[test]
fn rejected_control_out_is_stalled() {
    let (_device, mut client) = attach();
    assert_eq!(client.control_out(setup::vendor_reject(), &[]), Err(EPIPE));
    client.control_out(setup::vendor_accept(), &[]).unwrap();
}
