use dmds::StreamExt;
use libaccount::Phone;
use serde_json::json;

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
}
