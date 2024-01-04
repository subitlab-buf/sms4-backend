use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU64,
};

use axum::{
    extract::{Path, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use libaccount::{
    tag::{AsPermission, Tags},
    Phone, VerifyDescriptor,
};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{
        verify::{self, Captcha},
        Account, Permission, Tag, TagEntry, Unverified,
    },
    Error,
};

use crate::{Auth, Global};

#[derive(Deserialize)]
pub struct SendCaptchaReq {
    pub email: lettre::Address,
}

pub async fn send_captcha<Io: IoHandle>(
    State(Global {
        smtp_transport,
        worlds,
        config,
        test_cx,
    }): State<Global<Io>>,
    Json(SendCaptchaReq { email }): Json<SendCaptchaReq>,
) -> Result<(), Error> {
    let mut unverified = Unverified::new(email.to_string())?;
    let select = sd!(worlds.account, unverified.email_hash());
    if gd!(select, unverified.email_hash()).is_some() {
        return Err(Error::PermissionDenied);
    }

    let select = worlds
        .unverified_account
        .select(0, unverified.email_hash())
        .hint(unverified.email_hash());
    let mut iter = select.iter();
    while let Some(Ok(mut lazy)) = iter.next().await {
        if lazy.id() == unverified.email_hash() {
            if let Ok(val) = lazy.get_mut().await {
                if val.email() == unverified.email() {
                    val.send_captcha(&config.smtp, &smtp_transport, &test_cx)
                        .await?;
                    return Ok(());
                }
            }
        }
    }

    unverified
        .send_captcha(&config.smtp, &smtp_transport, &test_cx)
        .await?;
    worlds.unverified_account.insert(unverified).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct RegisterReq(pub VerifyDescriptor<Tag, verify::DescArgs>);

pub async fn register<Io: IoHandle>(
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(RegisterReq(desc)): Json<RegisterReq>,
) -> Result<(), Error> {
    let unverified = Unverified::new(desc.email.to_owned())?;
    let unverified = libaccount::Unverified::from(
        worlds
            .unverified_account
            .chunk_buf_of_data_or_load(&unverified)
            .await
            .map_err(|_| Error::UnverifiedAccountNotFound)?
            .remove(unverified.email_hash())
            .await
            .ok_or(Error::UnverifiedAccountNotFound)?,
    );
    match unverified.verify(desc) {
        Ok(verified) => worlds
            .account
            .try_insert(verified.into())
            .await
            .map_err(|_| Error::PermissionDenied),
        Err((err, unverified)) => {
            worlds.unverified_account.insert(unverified.into()).await?;
            Err(err)
        }
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: lettre::Address,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRes {
    pub id: u64,
    pub token: String,
    pub expire_at: Option<i64>,
}

pub async fn login<Io: IoHandle>(
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(LoginReq { email, password }): Json<LoginReq>,
) -> Result<Json<LoginRes>, Error> {
    let unverified = Unverified::new(email.to_string())?;
    let select = sd!(worlds.account, unverified.email_hash());
    let mut lazy =
        gd!(select, unverified.email_hash()).ok_or(Error::UsernameOrPasswordIncorrect)?;
    let (token, exp_time) = lazy.get_mut().await?.login(&password).map_err(|err| {
        if matches!(err, libaccount::Error::PasswordIncorrect) {
            Error::UsernameOrPasswordIncorrect
        } else {
            err.into()
        }
    })?;

    Ok(axum::Json(LoginRes {
        id: lazy.id(),
        token,
        expire_at: exp_time,
    }))
}

#[derive(Deserialize)]
pub struct SendResetPasswordCaptchaReq {
    pub email: lettre::Address,
}

pub async fn send_reset_password_captcha<Io: IoHandle>(
    State(Global {
        smtp_transport,
        worlds,
        config,
        test_cx,
    }): State<Global<Io>>,
    Json(SendResetPasswordCaptchaReq { email }): Json<SendResetPasswordCaptchaReq>,
) -> Result<(), Error> {
    let unverified = Unverified::new(email.to_string())?;
    let select = sd!(worlds.account, unverified.email_hash());
    let mut lazy = gd!(select, unverified.email_hash()).ok_or(Error::PermissionDenied)?;
    lazy.get_mut()
        .await?
        .req_reset_password(&config.smtp, &smtp_transport, &test_cx)
        .await
        .map_err(From::from)
}

#[derive(Deserialize)]
pub struct ResetPasswordReq {
    pub email: lettre::Address,
    pub captcha: Captcha,
    pub new_password: String,
}

pub async fn reset_password<Io: IoHandle>(
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(ResetPasswordReq {
        email,
        captcha,
        new_password,
    }): Json<ResetPasswordReq>,
) -> Result<(), Error> {
    let unverified = Unverified::new(email.to_string())?;
    let select = sd!(worlds.account, unverified.email_hash());
    let mut lazy = gd!(select, unverified.email_hash()).ok_or(Error::PermissionDenied)?;
    let account = lazy.get_mut().await?;
    account.reset_password(captcha, new_password)?;
    // Clear all tokens after reseting password
    account.clear_tokens();
    Ok(())
}

#[derive(Deserialize)]
pub struct ModifyReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub school_id: Option<String>,
    #[serde(default)]
    pub phone: Option<Phone>,

    /// Duration, as seconds.
    /// Zero means never expires.
    #[serde(default)]
    pub token_expire_duration: Option<u64>,
    #[serde(default)]
    pub password: Option<ModifyPasswordPart>,

    #[serde(default)]
    pub departments: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct ModifyPasswordPart {
    pub old: String,
    pub new: String,
}

pub async fn modify<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(mut req): Json<ModifyReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let mut lazy = va!(auth, select);
    let account = lazy.get_mut().await?;

    macro_rules! modify {
        ($($i:ident => $m:ident),*$(,)?) => { $(if let Some(v) = req.$i.take() { account.$m(v) })* };
    }
    modify! {
        name => set_name,
        school_id => set_school_id,
        phone => set_phone
    }
    if let Some(dur) = req.token_expire_duration.and_then(NonZeroU64::new) {
        account.set_token_expire_time(Some(dur.get()))
    }
    if let Some(ModifyPasswordPart { old, new }) = req.password.take() {
        if account.password_matches(&old) {
            account.set_password(new)
        } else {
            return Err(Error::UsernameOrPasswordIncorrect);
        }
    }
    if let Some(mut departments) = req.departments.take() {
        if let Some(t) = account.tags_mut().from_entry_mut(&TagEntry::Department) {
            t.clear()
        }
        if let Some(department) = departments.pop() {
            account.tags_mut().insert(Tag::Department(department));
            account
                .tags_mut()
                .from_entry_mut(&TagEntry::Department)
                .unwrap()
                .extend(departments.into_iter().map(Tag::Department))
        }
    }

    Ok(())
}

pub async fn logout<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let mut lazy = va!(auth, select);
    lazy.get_mut()
        .await?
        .logout(&auth.token)
        .map_err(From::from)
}

#[derive(Deserialize)]
pub struct SetPermissionsReq {
    pub target_account: u64,
    pub permissions: Vec<Permission>,
}

pub async fn set_permissions<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(SetPermissionsReq {
        target_account,
        permissions,
    }): Json<SetPermissionsReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let lazy = va!(auth, select => SetPermissions);
    let this = lazy.get().await?;
    let permissions: HashSet<_> = permissions.into_iter().map(From::from).collect();
    let legal_perms = permissions
        .intersection(
            this.tags()
                .from_entry(&TagEntry::Permission)
                .ok_or(Error::PermissionDenied)?,
        )
        .filter_map(Tag::as_permission)
        .copied();

    let select_t = sd!(worlds.account, target_account);
    let mut lazy_t = gd!(select_t, target_account).ok_or(Error::TargetAccountNotFound)?;
    let target = lazy_t.get_mut().await?;
    if this
        .tags()
        .from_entry(&TagEntry::Permission)
        .is_some_and(|p| {
            target
                .tags()
                .from_entry(&TagEntry::Permission)
                .map_or(true, |pt| pt.is_subset(p))
        })
    {
        target.tags_mut().initialize_permissions();
        *target
            .tags_mut()
            .from_entry_mut(&TagEntry::Permission)
            .unwrap() = legal_perms.into_iter().map(From::from).collect();
    }

    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum GetInfoRes {
    Simple {
        name: String,
        email: String,
        departments: Vec<String>,
    },
    Full {
        name: String,
        email: String,
        school_id: String,
        phone: Option<Phone>,
        tags: Tags<TagEntry, Tag>,
    },

    /// Requester owns the account.
    Owned {
        email: String,
        name: String,
        school_id: String,
        phone: Option<Phone>,

        /// Duration, as seconds.
        token_expire_duration: Option<NonZeroU64>,
        tags: Tags<TagEntry, Tag>,
    },
}

impl GetInfoRes {
    fn from_simple(account: &Account) -> Self {
        Self::Simple {
            name: account.name().to_owned(),
            email: account.email().to_owned(),
            departments: account
                .tags()
                .from_entry(&TagEntry::Department)
                .map_or(vec![], |set| {
                    set.iter()
                        .filter_map(|t| {
                            if let Tag::Department(d) = t {
                                Some(d.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                }),
        }
    }

    fn from_full(account: &Account) -> Self {
        Self::Full {
            name: account.name().to_owned(),
            email: account.email().to_owned(),
            school_id: account.school_id().to_owned(),
            phone: account.phone(),
            tags: account.tags().clone(),
        }
    }

    fn from_owned(account: &Account) -> Self {
        Self::Owned {
            email: account.email().to_owned(),
            name: account.name().to_owned(),
            school_id: account.school_id().to_owned(),
            phone: account.phone(),
            token_expire_duration: account.token_expire_time(),
            tags: account.tags().clone(),
        }
    }
}

pub async fn get_info<Io: IoHandle>(
    Path(target): Path<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<GetInfoRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => ViewSimpleAccount);
    let select = sd!(worlds.account, target);
    let lazy = gd!(select, target).ok_or(Error::TargetAccountNotFound)?;
    let account = lazy.get().await?;

    if auth.account == account.id() {
        Ok(Json(GetInfoRes::from_owned(account)))
    } else if this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ViewFullAccount))
    {
        Ok(Json(GetInfoRes::from_full(account)))
    } else {
        Ok(Json(GetInfoRes::from_simple(account)))
    }
}

#[derive(Deserialize)]
pub struct BulkGetInfoReq {
    pub accounts: Vec<u64>,
}

/// Bulk gets account info, returns a map from account id to simple account info,
/// as returning a full account info is expensive.
pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { accounts }): Json<BulkGetInfoReq>,
) -> Result<Json<HashMap<u64, GetInfoRes>>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ViewSimpleAccount);
    if let Some(last) = accounts.first().copied() {
        let mut select = worlds.account.select(0, last);
        for account in &accounts[1..] {
            select = select.plus(0, *account);
        }
        select = select.hints(accounts[..].into_iter().copied());
        let mut iter = select.iter();
        let mut res = HashMap::with_capacity(accounts.len());
        while let Some(Ok(lazy)) = iter.next().await {
            if accounts.contains(&lazy.id()) {
                if let Ok(account) = lazy.get().await {
                    res.insert(account.id(), GetInfoRes::from_simple(account));
                }
            }
        }
        Ok(Json(res))
    } else {
        Ok(Json(HashMap::new()))
    }
}
