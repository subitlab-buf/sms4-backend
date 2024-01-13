use std::{path::PathBuf, sync::Arc};

use axum::Router;
use dmds::{mem_io_handle::MemStorage, world};
use sms4_backend::config::Config;
use tokio::sync::Mutex;

use crate::Global;

fn router() -> (Global<MemStorage>, Router) {
    sms4_backend::IS_TEST.store(true, std::sync::atomic::Ordering::Release);
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
        resource_path: PathBuf::from(".test/resources"),
    };
    let state = Global {
        smtp_transport: Arc::new(config.smtp.to_transport().unwrap()),
        worlds: Arc::new(crate::Worlds {
            account: world!(MemStorage::new(), ipc!(16) => ..),
            unverified_account: world!(MemStorage::new(), ipc!(4) => ..),
            post: world!(
                MemStorage::new(),
                ipc!(16) => ..,
                368 / 4 => ..=367,
                ipc!(16) => ..,
                1 => ..2
            ),
            resource: world!(
                MemStorage::new(),
                ipc!(256) => ..,
                1 => ..2
            ),
            notification: world! {
                MemStorage::new(),
                ipc!(32) => ..,
                368 / 4 => ..=367
            },
        }),
        config: Arc::new(config),
        test_cx: Default::default(),
        resource_sessions: Arc::new(Mutex::new(sms4_backend::resource::UploadSessions::new())),
    };

    let router: Router<()> = crate::routing(Router::new()).with_state(state.clone());
    (state, router)
}

macro_rules! req {
    ($r:expr, $m:ident => $u:expr, $a:expr) => {{
        let mut b = Some(
            axum::http::Request::builder()
                .uri($u)
                .method(axum::http::Method::$m),
        );
        $a.append_to_req_builder(&mut b);
        let req = b.unwrap().body(axum::body::Body::empty()).unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
    ($r:expr, $m:ident => $u:expr, $a:expr, $b:expr => json) => {
        let mut b = Some(Request::builder().uri($u).method(axum::http::Method::$m));
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
    ($r:expr, $m:ident => $u:expr, $a:expr, $b:expr => bytes) => {{
        let mut b = Some(
            axum::http::Request::builder()
                .uri($u)
                .method(axum::http::Method::$m),
        );
        $a.append_to_req_builder(&mut b);
        let req = b.unwrap().body(axum::body::Body::from($b)).unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
    ($r:expr, $m:ident => $u:expr, $b:expr => json) => {{
        let req = axum::http::Request::builder()
            .uri($u)
            .method(axum::http::Method::$m)
            .header(
                axum::http::header::CONTENT_TYPE,
                mime::APPLICATION_JSON.as_ref(),
            )
            .body(axum::body::Body::from(serde_json::to_string(&$b).unwrap()))
            .unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
    ($r:expr, $m:ident => $u:expr, $b:expr => bytes) => {{
        let req = axum::http::Request::builder()
            .uri($u)
            .method(axum::http::Method::$m)
            .body(axum::body::Body::from($b))
            .unwrap();
        tower::ServiceExt::oneshot($r.clone(), req).await.unwrap()
    }};
}

macro_rules! acc_exp {
    (DCK, $($p:ident),*$(,)?) => {
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
        .into()
    };
    (MYG, $($p:ident),*$(,)?) => {
        libaccount::Account::new(
            "myg@pkuschool.edu.cn".to_owned(),
            "Genshine Owner".to_owned(),
            "?".to_owned(),
            Some(114514.into()),
            Default::default(),
            {
                use sms4_backend::account::Tag;
                let mut tags = libaccount::tag::Tags::new();
                $(tags.insert(Tag::Permission(sms4_backend::account::Permission::$p));)*
                tags.insert(Tag::Department("Party".to_owned()));
                tags
            },
            "123456".to_owned(),
            std::time::Duration::from_secs(60),
            siphasher::sip::SipHasher24::new(),
        )
        .into()
    };
    ($t:tt) => { acc_exp!($t,) }
}

macro_rules! p_json {
    ($r:expr) => {
        serde_json::from_slice(
            &http_body_util::BodyExt::collect($r.into_body())
                .await
                .unwrap()
                .to_bytes(),
        )
        .unwrap()
    };
    ($r:expr => $t:ty) => {{
        let val: $t = p_json!($r);
        val
    }};
}

mod account;
