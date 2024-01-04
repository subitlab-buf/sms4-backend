use dmds::StreamExt;
use libaccount::Phone;
use serde_json::json;
use sms4_backend::account::{Account, Permission, Tag, TagEntry};

use crate::{gd, handle::account::LoginRes, routes::*, sd, tests::router, va, Auth};

/// Send captcha and register an account.
#[tokio::test]
async fn creation() {
    let (state, route) = router();
    let res = req!(route => SEND_CAPTCHA,
        json!({ "email": "someone@beijing101.com" }) => json
    );
    assert!(!res.status().is_success());
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
            &[Permission::GetPubPost, Permission::Post]
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
    state.worlds.account.insert(acc_exp!(DCK)).await.unwrap();

    // Login with wrong password
    let res = req!(route => LOGIN,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "password": "123456",
        }) => json
    );
    assert!(!res.status().is_success());

    // Login successfully
    let res = req!(route => LOGIN,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "password": "shanlilinghuo",
        }) => json
    );
    assert!(res.status().is_success());
    let LoginRes { id, token, .. } = p_json!(res);
    let auth = Auth { account: id, token };

    let select = sd!(state.worlds.account, id);
    assert!(async { Ok(va!(auth, select)) }.await.is_ok());
}

#[tokio::test]
async fn logout() {
    let (state, route) = router();
    let mut account: Account = acc_exp!(DCK);
    let id = account.id();
    let (token0, _) = account.login("shanlilinghuo").unwrap();
    let (token1, _) = account.login("shanlilinghuo").unwrap();
    state.worlds.account.insert(account).await.unwrap();

    let auth_wrong = Auth {
        account: id,
        token: "wrong_token".to_owned(),
    };
    let auth = Auth {
        account: id,
        token: token0,
    };
    let wrong_res = req!(route => LOGOUT, auth_wrong, axum::body::Body::empty() => bytes);
    assert!(!wrong_res.status().is_success());
    let select = sd!(state.worlds.account, id);
    assert!(async { Ok(va!(auth, select)) }.await.is_ok());

    let res = req!(route => LOGOUT, auth, axum::body::Body::empty() => bytes);
    assert!(res.status().is_success());
    assert!(async { Ok(va!(auth, select)) }.await.is_err());
    let auth1 = Auth {
        account: id,
        token: token1,
    };
    assert!(async { Ok(va!(auth1, select)) }.await.is_ok());
}

#[tokio::test]
async fn reset_password() {
    let (state, route) = router();
    let account: Account = acc_exp!(DCK);
    let id = account.id();
    state.worlds.account.insert(account).await.unwrap();

    let res = req!(route => SEND_RESET_PASSWORD_CAPTCHA,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
        }) => json
    );
    assert!(res.status().is_success());
    let captcha = state
        .test_cx
        .captcha
        .lock()
        .await
        .expect("captcha not sent");
    let wrong_captcha = sms4_backend::account::verify::Captcha::from(captcha.into_inner() + 1);
    let res = req!(route => RESET_PASSWORD,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "captcha": wrong_captcha,
            "new_password": "nerd",
        }) => json
    );
    assert!(!res.status().is_success());
    let select = sd!(state.worlds.account, id);
    // Will dead lock if unblocked.
    {
        let lazy = gd!(select, id).unwrap();
        let account = lazy.get().await.unwrap();
        assert!(account.password_matches("shanlilinghuo"));
    }

    let res = req!(route => RESET_PASSWORD,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "captcha": captcha,
            "new_password": "NERD",
        }) => json
    );
    assert!(res.status().is_success());
    let lazy = gd!(select, id).unwrap();
    let account = lazy.get().await.unwrap();
    assert!(account.password_matches("NERD"));
}
