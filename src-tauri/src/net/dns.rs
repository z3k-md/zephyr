//! The one DNS query the monitor times: an A lookup built by hand, so it goes to the
//! system's resolver over UDP without any caching in between.

/// A standard query for `name`'s A record, with recursion wanted.
pub fn query(id: u16, name: &str) -> Vec<u8> {
    let mut packet = Vec::with_capacity(32);
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00]); // recursion desired
    packet.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // one question
    for label in name.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&[0, 1, 0, 1]); // A, IN
    packet
}

/// Whether `reply` answers query `id` without an error (it may still have no records).
pub fn answers(reply: &[u8], id: u16) -> bool {
    reply.len() >= 12
        && reply[..2] == id.to_be_bytes()
        && reply[2] & 0x80 != 0 // a response
        && reply[3] & 0x0F == 0 // no error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_standard_query() {
        let packet = query(0x1234, "example.com");
        assert_eq!(&packet[..4], &[0x12, 0x34, 0x01, 0x00]);
        assert_eq!(&packet[12..20], b"\x07example");
        assert_eq!(&packet[packet.len() - 5..], &[0, 0, 1, 0, 1]);
    }

    #[test]
    fn recognizes_its_own_answer() {
        let mut reply = query(7, "example.com");
        reply[2] |= 0x80;
        assert!(answers(&reply, 7));
        assert!(!answers(&reply, 8));
        reply[3] |= 0x03; // NXDOMAIN
        assert!(!answers(&reply, 7));
    }
}
