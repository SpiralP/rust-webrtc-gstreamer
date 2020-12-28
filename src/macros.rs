// upgrade weak reference or return
#[macro_export]
macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => {
                warn!("upgrade_weak failed");
                return $r;
            }
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

#[macro_export]
macro_rules! concat_spaces {
    ($($e:expr),+) => { concat_spaces!($($e,)+) };
    ($($e:expr,)+) => { concat!($($e, " ", )*) };
}

#[test]
fn test_concat_spaces() {
    assert_eq!(concat_spaces!("a", "b", "c"), "a b c ");
    assert_eq!(concat_spaces!("a", "b"), "a b ");
    assert_eq!(concat_spaces!("a"), "a ");
}
