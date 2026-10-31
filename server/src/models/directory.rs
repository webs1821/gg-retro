//! Authenticated, opt-in GG6 directory. Only explicit profile fields are public.
use crate::{core::AppState, models::DatabasePool};
use gg_protocol::{
  consts::GGStatus,
  packets::{ContactStatus, DirectoryPacket},
};
use sqlx::{FromRow, QueryBuilder, Sqlite};
use std::collections::BTreeMap;

#[derive(thiserror::Error, Debug)]
pub enum DirectoryError {
  #[error("Invalid public directory request")]
  Invalid,
  #[error("Directory database operation failed")]
  Database(#[from] sqlx::Error),
}
type Result<T> = std::result::Result<T, DirectoryError>;
const TEXT: [&str; 6] = [
  "firstname",
  "lastname",
  "nickname",
  "city",
  "familyname",
  "familycity",
];
const COLUMNS: &str = "uin,firstname,lastname,nickname,city,familyname,familycity,birthyear,gender";

#[derive(Default, FromRow)]
struct Profile {
  uin: u32,
  firstname: String,
  lastname: String,
  nickname: String,
  city: String,
  familyname: String,
  familycity: String,
  birthyear: u32,
  gender: u32, // WRITE/READ: 1 male, 2 female. SEARCH uses the reverse mapping.
}
fn folded(s: &str) -> String {
  s.trim()
    .to_lowercase()
    .chars()
    .map(|c| match c {
      'ą' => 'a',
      'ć' => 'c',
      'ę' => 'e',
      'ł' => 'l',
      'ń' => 'n',
      'ó' => 'o',
      'ś' => 's',
      'ź' | 'ż' => 'z',
      _ => c,
    })
    .collect()
}
fn number(s: &str, max: u32) -> Result<u32> {
  if s.is_empty() {
    return Ok(0);
  }
  if !s.bytes().all(|b| b.is_ascii_digit()) {
    return Err(DirectoryError::Invalid);
  }
  s.parse::<u32>()
    .ok()
    .filter(|n| *n <= max)
    .ok_or(DirectoryError::Invalid)
}
impl Profile {
  fn fields(&self, own: bool) -> Vec<(String, String)> {
    let mut result = vec![
      ("fmnumber".into(), self.uin.to_string()),
      ("firstname".into(), self.firstname.clone()),
      ("nickname".into(), self.nickname.clone()),
      ("city".into(), self.city.clone()),
      ("birthyear".into(), self.birthyear.to_string()),
    ];
    if own {
      result.extend([
        ("lastname".into(), self.lastname.clone()),
        ("familyname".into(), self.familyname.clone()),
        ("familycity".into(), self.familycity.clone()),
        ("gender".into(), self.gender.to_string()),
      ]);
    }
    result
  }
}
async fn read(pool: &DatabasePool, uin: u32) -> Result<Profile> {
  Ok(
    sqlx::query_as::<_, Profile>(&format!(
      "SELECT {COLUMNS} FROM public_profiles WHERE uin=?"
    ))
    .bind(uin)
    .fetch_optional(pool)
    .await?
    .unwrap_or(Profile {
      uin,
      ..Default::default()
    }),
  )
}
async fn write(pool: &DatabasePool, uin: u32, fields: &BTreeMap<String, String>) -> Result<()> {
  if fields.iter().any(|(k, v)| {
    !TEXT.contains(&k.as_str())
      && !["birthyear", "gender", "fmnumber"].contains(&k.as_str())
      && !v.is_empty()
  }) {
    return Err(DirectoryError::Invalid);
  }
  let mut p = read(pool, uin).await?;
  for (key, slot) in [
    ("firstname", &mut p.firstname),
    ("lastname", &mut p.lastname),
    ("nickname", &mut p.nickname),
    ("city", &mut p.city),
    ("familyname", &mut p.familyname),
    ("familycity", &mut p.familycity),
  ] {
    if let Some(v) = fields.get(key) {
      if v.chars().count() > 50 {
        return Err(DirectoryError::Invalid);
      }
      *slot = v.clone();
    }
  }
  if let Some(v) = fields.get("birthyear") {
    p.birthyear = number(v, 9999)?;
  }
  if let Some(v) = fields.get("gender") {
    p.gender = number(v, 2)?;
  }
  // The original form can keep a gender selection even after all text is cleared.
  if p.birthyear == 0
    && [
      &p.firstname,
      &p.lastname,
      &p.nickname,
      &p.city,
      &p.familyname,
      &p.familycity,
    ]
    .iter()
    .all(|v| v.is_empty())
  {
    sqlx::query("DELETE FROM public_profiles WHERE uin=?")
      .bind(uin)
      .execute(pool)
      .await?;
    return Ok(());
  }
  sqlx::query("INSERT INTO public_profiles (uin,firstname,lastname,nickname,city,familyname,familycity,birthyear,gender,first_key,last_key,nick_key,city_key,familyname_key,familycity_key)
    VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(uin) DO UPDATE SET
    firstname=excluded.firstname,lastname=excluded.lastname,nickname=excluded.nickname,city=excluded.city,
    familyname=excluded.familyname,familycity=excluded.familycity,birthyear=excluded.birthyear,gender=excluded.gender,
    first_key=excluded.first_key,last_key=excluded.last_key,nick_key=excluded.nick_key,city_key=excluded.city_key,
    familyname_key=excluded.familyname_key,familycity_key=excluded.familycity_key")
    .bind(uin).bind(&p.firstname).bind(&p.lastname).bind(&p.nickname).bind(&p.city).bind(&p.familyname).bind(&p.familycity)
    .bind(p.birthyear).bind(p.gender).bind(folded(&p.firstname)).bind(folded(&p.lastname)).bind(folded(&p.nickname))
    .bind(folded(&p.city)).bind(folded(&p.familyname)).bind(folded(&p.familycity)).execute(pool).await?;
  Ok(())
}
pub async fn handle(
  app: &AppState,
  viewer: u32,
  request: DirectoryPacket,
) -> Result<DirectoryPacket> {
  let mut reply = DirectoryPacket {
    kind: if request.kind == 3 { 5 } else { request.kind },
    seq: request.seq,
    records: vec![],
  };
  let fields: BTreeMap<_, _> = request
    .records
    .into_iter()
    .flatten()
    .map(|(k, v)| (k, v.trim().to_owned()))
    .collect();
  if request.kind != 3 {
    if let Some(value) = fields.get("fmnumber") {
      let target = number(value, 0xffffff)?;
      if target != 0 && target != viewer {
        return Err(DirectoryError::Invalid);
      }
    }
    match request.kind {
      1 => {
        write(app.db_pool(), viewer, &fields).await?;
        reply
          .records
          .push(vec![("fmnumber".into(), viewer.to_string())]);
      }
      2 => reply
        .records
        .push(read(app.db_pool(), viewer).await?.fields(true)),
      _ => return Err(DirectoryError::Invalid),
    }
    return Ok(reply);
  }
  // Unknown nonempty filters must never silently broaden a search. In particular,
  // email/phone searches cannot access account-registration data.
  if fields.iter().any(|(k, v)| {
    ![
      "fmnumber",
      "firstname",
      "lastname",
      "nickname",
      "city",
      "birthyear",
      "gender",
      "activeonly",
      "fmstart",
      "start",
    ]
    .contains(&k.as_str())
      && !v.is_empty()
  }) {
    return Ok(reply);
  }
  let value = |key: &str| fields.get(key).map(String::as_str).unwrap_or("");
  let start = number(
    if fields.contains_key("fmstart") {
      value("fmstart")
    } else {
      value("start")
    },
    0xffffff,
  )?;
  let target = number(value("fmnumber"), 0xffffff)?;
  let gender = number(value("gender"), 2)?;
  let active = number(value("activeonly"), 1)? == 1;
  let mut q = QueryBuilder::<Sqlite>::new(format!(
    "SELECT {COLUMNS} FROM public_profiles WHERE uin >= "
  ));
  q.push_bind(start);
  if target != 0 {
    q.push(" AND uin=").push_bind(target);
  }
  for (key, column, alternate) in [
    ("firstname", "first_key", None),
    ("nickname", "nick_key", None),
    ("lastname", "last_key", Some("familyname_key")),
    ("city", "city_key", Some("familycity_key")),
  ] {
    let term = folded(value(key));
    if !term.is_empty() {
      q.push(" AND (instr(")
        .push(column)
        .push(",")
        .push_bind(term.clone())
        .push(")>0");
      if let Some(other) = alternate {
        q.push(" OR instr(")
          .push(other)
          .push(",")
          .push_bind(term)
          .push(")>0");
      }
      q.push(")");
    }
  }
  let years: Vec<_> = value("birthyear").split_whitespace().collect();
  if !years.is_empty() {
    if years.len() > 2 {
      return Err(DirectoryError::Invalid);
    }
    let low = number(years[0], 9999)?;
    let high = number(years[years.len() - 1], 9999)?;
    if low > high {
      return Err(DirectoryError::Invalid);
    }
    if low != 0 || high != 0 {
      q.push(" AND birthyear BETWEEN ")
        .push_bind(low)
        .push(" AND ")
        .push_bind(high);
    }
  }
  if gender != 0 {
    q.push(" AND gender=").push_bind(3 - gender);
  }
  if active {
    let online = app.presence_hub().directory_online_for(viewer);
    if online.is_empty() {
      return Ok(reply);
    }
    q.push(" AND uin IN (");
    let mut list = q.separated(",");
    for uin in online {
      list.push_bind(uin);
    }
    list.push_unseparated(")");
  }
  q.push(" ORDER BY uin LIMIT 21");
  let profiles = q
    .build_query_as::<Profile>()
    .fetch_all(app.db_pool())
    .await?;
  let next = profiles.get(20).map(|p| p.uin).unwrap_or(0);
  for profile in profiles.into_iter().take(20) {
    let presence = app.presence_hub().find_for(profile.uin, viewer);
    if active
      && !matches!(
        presence.status.base(),
        GGStatus::Avail | GGStatus::AvailDescr | GGStatus::Busy | GGStatus::BusyDescr
      )
    {
      continue;
    }
    let status: ContactStatus = presence.into();
    let mut record = profile.fields(false);
    record.push(("fmstatus".into(), status.status.to_string()));
    reply.records.push(record);
  }
  if !reply.records.is_empty() || next != 0 {
    reply
      .records
      .push(vec![("nextstart".into(), next.to_string())]);
  }
  Ok(reply)
}
