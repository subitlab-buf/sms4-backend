use std::time::Duration;

use dmds::StreamExt;
use libaccount::Phone;
use serde_json::json;
use sms4_backend::account::{Account, Permission, Tag, TagEntry};

use crate::{routes::*, tests::router};

/// Send captcha and register an account.
#[tokio::test]
async fn creation() {
    let (state, route) = router();
    let res = req!(route => SEND_CAPTCHA,
        json!({ "email": "kongdechen2025@i.pkuschool.edu.cn" }) => json
    );
    assert!(res.status().is_success());
    let captcha = state
        .test_cx
        .captcha
        .lock()
        .await
        .expect("captcha not sent");

    // Verify the account.

    // Simulates a wrong captcha.
    let wrong_captcha = sms4_backend::account::verify::Captcha::from(captcha.into_inner() + 1);
    let res = req!(route => REGISTER,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "name": "Genshine Player",
            "school_id": "2525505",
            "phone": 12345678901_u64,
            "password": "shanlilinghuo",
            "captcha": wrong_captcha,
        }) => json
    );
    assert!(!res.status().is_success());

    let res = req!(route => REGISTER,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "name": "Genshine Player",
            "school_id": "2525505",
            "phone": 12345678901_u64,
            "password": "shanlilinghuo",
            "captcha": captcha,
            "tags": [
                {
                    "entry": "Permission",
                    "tag": "Op"
                },
                {
                    "entry": "Department",
                    "tag": "SubIT"
                },
                {
                    "entry": "Department",
                    "tag": "击剑批"
                },
                {
                    "entry": "House",
                    "tag": 5
                }
            ]
        }) => json
    );
    assert!(res.status().is_success());

    let select = state.worlds.account.select_all();
    let mut iter = select.iter();
    let lazy = iter.next().await.unwrap().unwrap();
    let account = lazy.get().await.unwrap();
    assert_eq!(account.email(), "kongdechen2025@i.pkuschool.edu.cn");
    assert!(account.password_matches("shanlilinghuo"));
    assert_eq!(account.phone(), Some(Phone::new(86, 12345678901)));
    assert_eq!(
        account.tags().from_entry(&TagEntry::Permission),
        Some(
            &[Permission::GetPubPosts, Permission::Post]
                .map(Tag::Permission)
                .into()
        )
    );
    assert_eq!(
        account.tags().from_entry(&TagEntry::Department),
        Some(
            &["SubIT", "击剑批"]
                .map(|s| Tag::Department(s.to_owned()))
                .into()
        )
    )
}

#[tokio::test]
async fn login() {
    let (state, route) = router();
    init_acc!(state, DCK);
}
