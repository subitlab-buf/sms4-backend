use std::collections::HashMap;

use dmds::StreamExt;
use libaccount::Phone;
use serde_json::json;
use sms4_backend::account::{Account, Permission, Tag, TagEntry};

use crate::{gd, handle::account::LoginRes, routes::*, sd, tests::router, va, Auth};

/// Send captcha and register an account.
#[tokio::test]
async fn creation() {
    let (state, route) = router();
    let res = req!(route, POST => SEND_CAPTCHA,
        json!({ "email": "someone@beijing101.com" }) => json
    );
    assert!(!res.status().is_success());
    let res = req!(route, POST => SEND_CAPTCHA,
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
    let res = req!(route, PUT => REGISTER,
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

    let res = req!(route, PUT => REGISTER,
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
            &[
                Permission::GetPubPost,
                Permission::Post,
                Permission::UploadResource,
                Permission::ViewSimpleAccount,
                Permission::GetPubNotifications,
            ]
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
    let res = req!(route, POST => LOGIN,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "password": "123456",
        }) => json
    );
    assert!(!res.status().is_success());

    // Login successfully
    let res = req!(route, POST => LOGIN,
        json!({
            "email": "kongdechen2025@i.pkuschool.edu.cn",
            "password": "shanlilinghuo",
        }) => json
    );
    assert!(res.status().is_success());
    let LoginRes { id, token, .. } = p_json!(res);
    let auth = Auth {
        account: id.0,
        token,
    };

    let select = sd!(state.worlds.account, id.0);
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
    let wrong_res = req!(route, POST => LOGOUT, auth_wrong);
    assert!(!wrong_res.status().is_success());
    let select = sd!(state.worlds.account, id);
    assert!(async { Ok(va!(auth, select)) }.await.is_ok());

    let res = req!(route, POST => LOGOUT, auth);
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

    let res = req!(route, POST => SEND_RESET_PASSWORD_CAPTCHA,
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
    let res = req!(route, PATCH => RESET_PASSWORD,
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

    let res = req!(route, PATCH => RESET_PASSWORD,
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

#[tokio::test]
async fn get_info() {
    let (state, route) = router();
    let mut account: Account = acc_exp!(DCK, ViewSimpleAccount);
    let (token, _) = account.login("shanlilinghuo").unwrap();
    let id = account.id();
    state.worlds.account.insert(account).await.unwrap();

    let res = req!(route, GET => format!("/account/get/{id}"), Auth { account: id, token: token.to_owned() });
    assert!(res.status().is_success());
    let info: crate::handle::account::Info = p_json!(res);
    assert!(matches!(info, crate::handle::account::Info::Owned { .. }));

    let mut another: Account = acc_exp!(MYG, ViewFullAccount, ViewSimpleAccount);
    let (another_token, _) = another.login("123456").unwrap();
    let another_id = another.id();
    state.worlds.account.insert(another).await.unwrap();

    let res = req!(route, GET => format!("/account/get/{another_id}"), Auth { account: id, token: token.to_owned() });
    assert!(res.status().is_success());
    let info: crate::handle::account::Info = p_json!(res);
    assert!(matches!(info, crate::handle::account::Info::Simple { .. }));

    let res = req!(route, GET => format!("/account/get/{id}"), Auth { account: another_id, token: another_token });
    assert!(res.status().is_success());
    let info: crate::handle::account::Info = p_json!(res);
    assert!(matches!(info, crate::handle::account::Info::Full { .. }));

    let res = req!(route, POST => BULK_GET_ACCOUNT_INFO, Auth { account: id, token: token.to_owned() },
        json!({
            "ids": [id, another_id],
        }) => json
    );
    assert!(res.status().is_success());
    let infos: HashMap<u64, crate::handle::account::Info> = p_json!(res);
    assert_eq!(infos.len(), 2);
}

#[tokio::test]
async fn modify() {
    let (state, route) = router();
    let mut account: Account = acc_exp!(DCK, ViewSimpleAccount);
    let (token, _) = account.login("shanlilinghuo").unwrap();
    let id = account.id();
    state.worlds.account.insert(account).await.unwrap();

    let res = req!(route, PATCH => MODIFY_ACCOUNT,
        Auth { account: id, token: token.to_owned() },
        json!({
            "name": "Genshine Enjoyer",
            "token_expire_duration": 3600,
            "password": {
                "old": "shanlilinghuo",
                "new": "666666",
            },
            "departments": [
                "Tianma",
            ],
        }) => json
    );
    assert!(res.status().is_success());
    let select = sd!(state.worlds.account, id);
    let lazy = gd!(select, id).unwrap();
    let account = lazy.get().await.unwrap();
    assert_eq!(account.name(), "Genshine Enjoyer");
    assert!(account.password_matches("666666"));
    assert_eq!(
        account.tags().from_entry(&TagEntry::Department),
        Some(&["Tianma"].map(|s| Tag::Department(s.to_owned())).into())
    );
}
