//! The state of a row of Windows' TCP tables (`GetExtendedTcpTable`), pure so it is tested on every OS.
//!
//! Whether a row listens comes from the table it was read from, not from its `dwState`: Microsoft documents
//! `TCP_TABLE_OWNER_PID_LISTENER` as "all listening (receiving only) TCP endpoints" and `TCP_TABLE_OWNER_PID_CONNECTIONS`
//! as "all connected TCP endpoints" (learn.microsoft.com, TCP_TABLE_CLASS), while newer Windows 11 builds report a
//! listener's `dwState` as 0, a value MIB_TCP_STATE does not define. Seen on Dyno's Windows CI runner from 2026-10-29
//! (job 22533), right after it installed the optional preview update KB5127653 (Windows 11 26H1, build 28000.3365,
//! released 2026-10-28); also reported for build 26340.9596 (Hyact/workbuddy-switch-plus#2); build 26300 reports 2.
//! Microsoft's release notes for KB5127653 (Release Preview build 28000.3351) mention no change to the TCP tables, so
//! the cause is not documented; users get these builds too, so state 0 is handled, not waited out.

use dyno_core::network::SocketState;

/// The table a row was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpTable {
    /// `TCP_TABLE_OWNER_PID_LISTENER`: listening endpoints only.
    Listener,
    /// `TCP_TABLE_OWNER_PID_CONNECTIONS`: connected (non-listening) endpoints only.
    Connections,
}

/// The MIB_TCP_STATE name of `state`, or `UNKNOWN` for a value Windows does not document.
pub fn mib_name(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RECEIVED",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT_1",
        7 => "FIN_WAIT_2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        // MIB_TCP_STATE_RESERVED: bound but not (yet) listening or connected; `netstat -q` shows it as BOUND.
        100 => "BOUND",
        _ => "UNKNOWN",
    }
}

/// The buffer size to ask for after Windows said a table needs `reported` bytes: an eighth more, so a table that grows
/// again before the next read still fits, but never more than `cap`; `None` when the table itself is over the cap.
pub fn next_table_size(reported: u32, cap: u32) -> Option<u32> {
    (reported <= cap).then(|| reported.saturating_add(reported / 8).min(cap))
}

/// The error for a table Windows would not give (`what` names it), with the Win32 code.
pub fn table_error(what: &str, code: u32) -> String {
    format!("Windows would not list the {what} (error {code}, {code:#010X}).")
}

/// The state of a row read from `table`.
pub fn row_state(table: TcpTable, state: u32) -> SocketState {
    match (table, state) {
        (TcpTable::Listener, _) | (TcpTable::Connections, 2) => SocketState::Listen,
        (TcpTable::Connections, 3) => SocketState::SynSent,
        (TcpTable::Connections, 4) => SocketState::SynReceived,
        (TcpTable::Connections, 5) => SocketState::Established,
        (TcpTable::Connections, 6 | 7) => SocketState::FinWait,
        (TcpTable::Connections, 8) => SocketState::CloseWait,
        (TcpTable::Connections, 9) => SocketState::Closing,
        (TcpTable::Connections, 10) => SocketState::LastAck,
        (TcpTable::Connections, 11) => SocketState::TimeWait,
        (TcpTable::Connections, _) => SocketState::Other,
    }
}

/// The state name shown for a row read from `table`: `LISTEN` for a listener, else the MIB_TCP_STATE name, or
/// `UNKNOWN (n)` for a value Windows does not document, so what the OS said is not lost.
pub fn row_name(table: TcpTable, state: u32) -> String {
    match (table, mib_name(state)) {
        (TcpTable::Listener, _) => "LISTEN".into(),
        (TcpTable::Connections, "UNKNOWN") => format!("UNKNOWN ({state})"),
        (TcpTable::Connections, name) => name.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::network::SocketState;

    /// Regression (Windows CI runs 7145 to 7174): on newer Windows 11 builds a listener's row carries `dwState` 0, which
    /// MIB_TCP_STATE does not define (CI job 22533; also reported for build 26340.9596, Hyact/workbuddy-switch-plus#2).
    /// Whether a row listens comes from the table it was read from (`TCP_TABLE_OWNER_PID_LISTENER`), not from that field.
    #[test]
    fn a_listener_with_state_zero_is_a_listener_and_a_connection_with_it_is_unknown() {
        assert_eq!(row_state(TcpTable::Listener, 0), SocketState::Listen);
        assert_eq!(row_name(TcpTable::Listener, 0), "LISTEN");
        assert_eq!(row_state(TcpTable::Listener, 2), SocketState::Listen);
        assert_eq!(row_state(TcpTable::Connections, 0), SocketState::Other);
        assert_eq!(row_name(TcpTable::Connections, 0), "UNKNOWN (0)");
    }

    /// Review P2 (2026-10-30): a table that grew between the size query and the read is asked for with headroom, so a
    /// busy machine does not use up the retries; never past the cap.
    #[test]
    fn a_table_that_grew_is_read_again_with_headroom_up_to_the_cap() {
        assert_eq!(next_table_size(800, 1 << 20), Some(900));
        assert_eq!(next_table_size(0, 1 << 20), Some(0));
        assert_eq!(next_table_size(1000, 1100), Some(1100), "headroom stops at the cap");
        assert_eq!(next_table_size(2000, 1100), None, "a table over the cap is not read");
        assert_eq!(next_table_size(u32::MAX, u32::MAX), Some(u32::MAX));
    }

    /// Review P2 (2026-10-30): a table Windows would not give is an error that names it and the Win32 code, never
    /// silently left out of the list.
    #[test]
    fn a_table_that_cannot_be_read_is_named_with_its_code() {
        assert_eq!(
            table_error("listening TCP sockets (IPv6)", 122),
            "Windows would not list the listening TCP sockets (IPv6) (error 122, 0x0000007A)."
        );
    }

    #[test]
    fn connection_states_keep_their_names() {
        assert_eq!(row_state(TcpTable::Connections, 5), SocketState::Established);
        assert_eq!(row_name(TcpTable::Connections, 5), "ESTABLISHED");
        assert_eq!(row_state(TcpTable::Connections, 7), SocketState::FinWait);
        assert_eq!(row_state(TcpTable::Connections, 11), SocketState::TimeWait);
        assert_eq!(row_state(TcpTable::Connections, 12), SocketState::Other);
        assert_eq!(row_name(TcpTable::Connections, 100), "BOUND");
        assert_eq!(row_name(TcpTable::Connections, 99), "UNKNOWN (99)");
        assert_eq!(mib_name(2), "LISTEN");
    }
}
