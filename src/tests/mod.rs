use std::sync::Arc;

use axum::Router;
use dmds::{mem_io_handle::MemStorage, world};
use sms4_backend::config::Config;

use crate::Global;

fn router() -> (Global<MemStorage>, Router) {
    sms4_backend::IS_TEST.store(true, std::sync::atomic::Ordering::Release);

    use crate::{handle, routes::*};
    use axum::routing::{get, post};
    use lettre::transport::smtp::authentication::Mechanism;

    let config = Config {
        smtp: sms4_backend::config::SMTP {
            server: "smtp-mail.outlook.com".to_owned(),
            port: Some(587),
            address: "someone@pkuschool.edu.cn".parse().unwrap(),
            username: "".to_owned(),
            password: "".to_owned(),
            auth: vec![Mechanism::Plain, Mechanism::Login],
            encrypt: sms4_backend::config::SmtpEncryption::StartTls,
        },
        db_path: Default::default(),
        port: 8080,
    };
    let state = Global {
        smtp_transport: Arc::new(config.smtp.to_transport().unwrap()),
        worlds: Arc::new(crate::Worlds {
            account: world!(MemStorage::new(), 1152921504606846976 | ..=u64::MAX),
            unverified_account: world!(MemStorage::new(), 4611686018427387904 | ..=u64::MAX),
        }),
        config: Arc::new(config),
        test_cx: Default::default(),
    };

    let router: Router<()> = Router::new()
        .route(SEND_CAPTCHA, post(handle::account::send_captcha))
        .route(REGISTER, post(handle::account::register))
        .route(LOGIN, post(handle::account::login))
        .route(RESET_PASSWORD, post(handle::account::reset_password))
        .route(SELF_ACCOUNT_INFO, post(handle::account::self_info))
        .route(MODIFY_ACCOUNT, post(handle::account::modify))
        .route(LOGOUT, post(handle::account::logout))
        .route(SET_PERMISSIONS, post(handle::account::set_permissions))
        .with_state(state.clone());

    (state, router)
}

macro_rules! req {
    ($r:expr => $u:expr, $a:expr) => {{
        let mut b = Some(Request::builder().uri($u).method(axum::http::Method::GET));
        $a.append_to_req_builder(&mut $b);
        let req = b.unwrap().body(Body::empty()).unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
    ($r:expr => $u:expr, $a:expr, $b:expr => json) => {
        let mut b = Some(Request::builder().uri($u).method(axum::http::Method::POST));
        $a.append_to_req_builder(&mut $b);
        let req = b
            .unwrap()
            .header(
                axum::http::header::CONTENT_TYPE,
                mime::APPLICATION_JSON.as_ref(),
            )
            .body(serde_json::to_string(&$b).unwrap())
            .unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    };
    ($r:expr => $u:expr, $a:expr, $b:expr => bytes) => {
        let mut b = Some(Request::builder().uri($u).method(axum::http::Method::POST));
        $a.append_to_req_builder(&mut $b);
        let req = b.unwrap().body(axum::body::Body::from($b)).unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    };
    ($r:expr => $u:expr, $b:expr => json) => {{
        let req = axum::http::Request::builder()
            .uri($u)
            .method(axum::http::Method::POST)
            .header(
                axum::http::header::CONTENT_TYPE,
                mime::APPLICATION_JSON.as_ref(),
            )
            .body(axum::body::Body::from(serde_json::to_string(&$b).unwrap()))
            .unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
}

macro_rules! init_acc {
    ($s:expr, DCK, $($p:ident),*$(,)?) => {
        $s
        .worlds
        .account
        .insert(
            libaccount::Account::new(
                "kongdechen2025@i.pkuschool.edu.cn".to_owned(),
                "Genshine Player".to_owned(),
                2525505.to_string(),
                Some(12345678901.into()),
                Default::default(),
                {
                    use sms4_backend::account::Tag;
                    let mut tags = libaccount::tag::Tags::new();
                    $(tags.insert(Tag::Permission(sms4_backend::account::Permission::$p));)*
                    tags.insert(Tag::Department("SubIT".to_owned()));
                    tags.insert(Tag::Department("击剑批".to_owned()));
                    tags.insert(Tag::House(libaccount::House::MingDe));
                    tags
                },
                "shanlilinghuo".to_owned(),
                std::time::Duration::from_secs(60),
                siphasher::sip::SipHasher24::new(),
            )
            .into(),
        )
        .await
        .unwrap();
    };
    ($s:expr, $t:tt) => { init_acc!($s, $t,) }
}

mod account;
