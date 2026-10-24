use paddock_live::{Channel, ChannelError};

#[test]
fn a_channel_is_a_short_lowercase_name_and_names_its_event() {
    let c = Channel::new("overview-2").unwrap();
    assert_eq!(c.as_str(), "overview-2");
    assert_eq!(c.event(), "paddock://live/overview-2");
}

#[test]
fn bad_names_say_why() {
    assert_eq!(Channel::new(""), Err(ChannelError::Length));
    assert_eq!(Channel::new(&"a".repeat(33)), Err(ChannelError::Length));
    assert!(Channel::new(&"a".repeat(32)).is_ok());
    assert_eq!(Channel::new("1abc"), Err(ChannelError::Start));
    assert_eq!(Channel::new("-abc"), Err(ChannelError::Start));
    assert_eq!(Channel::new("Abc"), Err(ChannelError::Start));
    assert_eq!(Channel::new("a/b"), Err(ChannelError::Character));
    assert_eq!(Channel::new("a b"), Err(ChannelError::Character));
    assert_eq!(Channel::new("aB"), Err(ChannelError::Character));
    for e in [
        ChannelError::Length,
        ChannelError::Start,
        ChannelError::Character,
    ] {
        assert!(!e.to_string().is_empty());
    }
}
