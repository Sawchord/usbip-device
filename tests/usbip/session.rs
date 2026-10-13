//! Several URBs in flight, URBs the host gives up on, and hosts that come and go

use crate::harness::{attach, pattern, setup, Client, Reply, BULK_IN, CMD_ECHO, ECONNRESET};

/// Control URBs submitted together are answered in order, each with its own reply
#[test]
fn queued_control_transfers() {
    let (_device, mut client) = attach();
    let first = client.send(0, setup::vendor_read(3, 64), None, 64, 0);
    let second = client.send(0, setup::vendor_accept(), Some(&[]), 0, 0);
    let third = client.send(0, setup::vendor_read(70, 255), None, 255, 0);
    assert_eq!(client.complete(first), Ok(pattern(3)));
    assert_eq!(client.complete(second), Ok(vec![]));
    assert_eq!(client.complete(third), Ok(pattern(70)));
}

/// Replies that fill the host's buffer exactly need no zero-length packet, so one sent
/// anyway must not answer the request queued behind them. Repeated, because whether the
/// device reaches that packet before the next setup depends on when the requests arrive.
#[test]
fn queued_exact_reads() {
    let (_device, mut client) = attach();
    for _ in 0..20 {
        let exact = client.send(0, setup::vendor_read(64, 64), None, 64, 0);
        let short = client.send(0, setup::vendor_read(5, 64), None, 64, 0);
        let out = client.send(0, setup::vendor_accept(), Some(&[]), 0, 0);
        assert_eq!(client.complete(exact), Ok(pattern(64)));
        assert_eq!(client.complete(short), Ok(pattern(5)));
        assert_eq!(client.complete(out), Ok(vec![]));
    }
}

/// A control OUT with a data stage queued behind a reply that fills the host's buffer
/// exactly, which must not see an IN completion meant for the reply
#[test]
fn queued_exact_read_then_control_out() {
    let (_device, mut client) = attach();
    for _ in 0..20 {
        let exact = client.send(0, setup::vendor_read(64, 64), None, 64, 0);
        let store = client.send(0, setup::vendor_store(10), Some(&pattern(10)), 0, 0);
        assert_eq!(client.complete(exact), Ok(pattern(64)));
        assert_eq!(client.complete(store), Ok(vec![]));
    }
    assert_eq!(
        client.control_in(setup::vendor_load(255)).unwrap(),
        pattern(10)
    );
}

/// Unlinking the control transfer in progress drops the rest of its data stage, which
/// must not answer the transfer queued behind it. The device sends one packet per poll,
/// so the unlink, sent right behind, arrives while the 128-byte reply is under way.
#[test]
fn unlink_current_control_transfer() {
    let (_device, mut client) = attach();
    let first = client.send(0, setup::vendor_read(128, 255), None, 255, 0);
    let second = client.send(0, setup::vendor_read(5, 64), None, 64, 0);
    let unlink = client.unlink(first);
    let mut unlinked = false;
    loop {
        match client.reply() {
            Reply::Unlink { seqnum, status } => {
                assert_eq!(seqnum, unlink);
                unlinked = status == ECONNRESET;
            }
            Reply::Submit {
                seqnum,
                status,
                data,
            } if seqnum == first => {
                // The transfer finished before the unlink arrived
                assert_eq!((status, data), (0, pattern(128)));
            }
            Reply::Submit {
                seqnum,
                status,
                data,
            } => {
                assert_eq!(seqnum, second);
                assert_eq!((status, data), (0, pattern(5)));
                break;
            }
        }
    }
    assert!(
        unlinked,
        "the unlink did not catch the transfer in progress"
    );
}

/// Unlinking a control OUT before the device has read all of its data drops the rest, so
/// the device never acts on the request
#[test]
fn unlink_current_control_out() {
    let (_device, mut client) = attach();
    let store = client.send(0, setup::vendor_store(100), Some(&pattern(100)), 0, 0);
    let read = client.send(0, setup::vendor_read(5, 64), None, 64, 0);
    let unlink = client.unlink(store);
    let mut unlinked = false;
    loop {
        match client.reply() {
            Reply::Unlink { seqnum, status } => {
                assert_eq!(seqnum, unlink);
                unlinked = status == ECONNRESET;
            }
            // The transfer finished before the unlink arrived
            Reply::Submit { seqnum, status, .. } if seqnum == store => assert_eq!(status, 0),
            Reply::Submit {
                seqnum,
                status,
                data,
            } => {
                assert_eq!(seqnum, read);
                assert_eq!((status, data), (0, pattern(5)));
                break;
            }
        }
    }
    if unlinked {
        assert_eq!(client.control_in(setup::vendor_load(255)), Ok(vec![]));
    }
}

/// An unlinked URB is never answered, and the data meant for the host goes to the next one
#[test]
fn unlink_pending_urb() {
    let (_device, mut client) = attach();
    let read = client.send(BULK_IN, [0; 8], None, 64, 0);
    let unlink = client.unlink(read);
    assert_eq!(
        client.reply(),
        Reply::Unlink {
            seqnum: unlink,
            status: ECONNRESET
        }
    );
    client.bulk_out(&[CMD_ECHO, 6]).unwrap();
    assert_eq!(client.bulk_in(64).unwrap(), [CMD_ECHO, 6]);
}

/// Unlinking a URB that was already answered is not an error
#[test]
fn unlink_answered_urb() {
    let (_device, mut client) = attach();
    let read = client.send(0, setup::vendor_read(3, 64), None, 64, 0);
    assert_eq!(client.complete(read), Ok(pattern(3)));
    let unlink = client.unlink(read);
    assert_eq!(
        client.reply(),
        Reply::Unlink {
            seqnum: unlink,
            status: 0
        }
    );
}

/// URBs of a host that disconnected are dropped with it, so the next host starts clean
#[test]
fn reconnect() {
    let (_device, mut client) = attach();
    client.send(BULK_IN, [0; 8], None, 64, 0);
    drop(client);
    let mut client = Client::connect();
    client.bulk_out(&[CMD_ECHO, 8]).unwrap();
    assert_eq!(client.bulk_in(64).unwrap(), [CMD_ECHO, 8]);
}
