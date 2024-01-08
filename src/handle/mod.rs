/// Selects an item.
#[macro_export]
macro_rules! sd {
    ($w:expr, $id:expr) => {
        $w.select(0, $id).hint($id)
    };
}

/// Gets an item from selection.
#[macro_export]
macro_rules! gd {
    ($s:expr, $id:expr) => {{
        let mut iter = $s.iter();
        let mut lazy = None;
        while let Some(Ok(l)) = dmds::StreamExt::next(&mut iter).await {
            if l.id() == $id {
                lazy = Some(l);
            }
        }
        lazy
    }};
}

/// Validates an account.
#[macro_export]
macro_rules! va {
        ($a:expr, $s:expr => $($p:ident),*$(,)?) => {{
            let lazy = gd!($s, $a.account).ok_or(crate::Error::PermissionDenied)?;
            let a = lazy.get().await?;
            if a.is_token_valid(&$a.token) {
                let _tags = a.tags();
                if !($(_tags.contains_permission(&sms4_backend::account::Tag::Permission(sms4_backend::account::Permission::$p)) &&)* true) {
                    return Err($crate::Error::PermissionDenied);
                }
            } else {
                return Err($crate::Error::LibAccount(libaccount::Error::InvalidToken));
            }
            lazy
        }};
        ($a:expr, $s:expr) => {
            va!($a, $s =>)
        }
    }

pub mod account;
pub mod notification;
pub mod post;
pub mod resource;
