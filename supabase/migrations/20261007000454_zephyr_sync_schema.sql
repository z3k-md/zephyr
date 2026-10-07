-- Zephyr sync: devices, key envelopes and end-to-end encrypted records.
-- The server only ever holds ciphertext; keys are made and opened on devices.

create schema zephyr;
create schema zephyr_private;
comment on schema zephyr is 'Zephyr launcher sync (notes, clipboard). Ciphertext only. Expose via Data API; enforce ownership with RLS.';
comment on schema zephyr_private is 'Zephyr-only non-exposed helpers (triggers). Not listed in api.schemas; not granted to client roles.';

-- Devices: one per signed-in install. public_key is the device's X25519 key; the account key
-- reaches a device only as an envelope sealed to it, after another device approves it.
create table zephyr.devices (
  id             uuid primary key default gen_random_uuid(),
  user_id        uuid not null default auth.uid() references auth.users on delete cascade,
  platform       text not null check (platform in ('macos', 'windows', 'linux')),
  display_name   text not null check (char_length(display_name) between 1 and 64 and display_name !~ '[\n\r]'),
  public_key     bytea not null check (octet_length(public_key) = 32),
  public_key_alg text not null default 'X25519' check (public_key_alg = 'X25519'),
  created_at     timestamptz not null default now(),
  last_seen_at   timestamptz not null default now(),
  revoked_at     timestamptz
);
comment on table zephyr.devices is 'Device labels and X25519 public keys only. No key material, no content.';
create index devices_user_id_idx on zephyr.devices (user_id);

-- Envelopes: the account key (AK) sealed once per approved device, plus one under the recovery key.
create table zephyr.key_envelopes (
  id                   uuid primary key default gen_random_uuid(),
  user_id              uuid not null default auth.uid() references auth.users on delete cascade,
  key_gen              int not null check (key_gen >= 1),
  kind                 text not null check (kind in ('device', 'recovery')),
  recipient_device_id  uuid references zephyr.devices on delete cascade,
  sender_device_id     uuid references zephyr.devices on delete set null,
  ephemeral_public_key bytea check (octet_length(ephemeral_public_key) = 32),
  nonce                bytea not null check (octet_length(nonce) = 24),
  ciphertext           bytea not null check (octet_length(ciphertext) between 48 and 1024),
  created_at           timestamptz not null default now(),
  check ((kind = 'device') = (recipient_device_id is not null)),
  check ((kind = 'device') = (ephemeral_public_key is not null)),
  unique nulls not distinct (user_id, key_gen, kind, recipient_device_id)
);
comment on table zephyr.key_envelopes is 'Account key wrapped per device (X25519 + XChaCha20-Poly1305) or under the recovery key. Ciphertext only; append-only for clients.';
create index key_envelopes_recipient_idx on zephyr.key_envelopes (recipient_device_id);
create index key_envelopes_sender_idx on zephyr.key_envelopes (sender_device_id);

-- Records: one sealed note or clipboard item per row. seq is the server's change order.
create sequence zephyr.records_seq;
create table zephyr.records (
  user_id    uuid not null default auth.uid() references auth.users on delete cascade,
  collection text not null check (collection in ('note', 'clip')),
  id         uuid not null,
  seq        bigint not null, -- set by zephyr_private.stamp_record()
  key_gen    int not null check (key_gen >= 1),
  sealed     bytea check (octet_length(sealed) <= 1048576),
  deleted    boolean not null default false,
  device_id  uuid references zephyr.devices on delete set null,
  updated_at timestamptz not null default now(),
  primary key (user_id, collection, id),
  check (deleted = (sealed is null))
);
comment on table zephyr.records is 'XChaCha20-Poly1305 sealed notes and clipboard items. Deleted rows are tombstones with no ciphertext.';
create index records_user_seq_idx on zephyr.records (user_id, seq);
create index records_device_idx on zephyr.records (device_id);

-- Every write gets a fresh seq and timestamp. The per-user lock makes seq order match commit
-- order, so a device pulling "seq > cursor" can never skip a row committed late.
create function zephyr_private.stamp_record() returns trigger
language plpgsql set search_path = '' as $$
begin
  perform pg_advisory_xact_lock(hashtextextended(new.user_id::text, 0));
  new.seq := nextval('zephyr.records_seq');
  new.updated_at := now();
  return new;
end;
$$;
create trigger records_stamp before insert or update on zephyr.records
  for each row execute function zephyr_private.stamp_record();

-- Push one record against the seq it was based on (0 = new). Returns the new seq, or raises
-- 'conflict' (SQLSTATE P0409) with the current seq in DETAIL when someone else changed it first.
create function zephyr.push_record(
  p_collection text, p_id uuid, p_base_seq bigint, p_key_gen int,
  p_sealed bytea, p_deleted boolean, p_device_id uuid
) returns bigint
language plpgsql security invoker set search_path = '' as $$
declare
  v_seq bigint;
begin
  if p_base_seq = 0 then
    insert into zephyr.records (collection, id, key_gen, sealed, deleted, device_id)
    values (p_collection, p_id, p_key_gen, p_sealed, p_deleted, p_device_id)
    on conflict (user_id, collection, id) do nothing
    returning seq into v_seq;
  else
    update zephyr.records
       set key_gen = p_key_gen, sealed = p_sealed, deleted = p_deleted, device_id = p_device_id
     where user_id = (select auth.uid()) and collection = p_collection and id = p_id
       and seq = p_base_seq
    returning seq into v_seq;
  end if;
  if v_seq is null then
    select seq into v_seq from zephyr.records
     where user_id = (select auth.uid()) and collection = p_collection and id = p_id;
    raise exception using errcode = 'P0409', message = 'conflict', detail = coalesce(v_seq, 0)::text;
  end if;
  return v_seq;
end;
$$;

-- Row-level security: every row belongs to auth.uid().
alter table zephyr.devices enable row level security;
alter table zephyr.key_envelopes enable row level security;
alter table zephyr.records enable row level security;

create policy devices_select_own on zephyr.devices for select to authenticated
  using (user_id = (select auth.uid()));
create policy devices_insert_own on zephyr.devices for insert to authenticated
  with check (user_id = (select auth.uid()) and revoked_at is null);
create policy devices_update_own on zephyr.devices for update to authenticated
  using (user_id = (select auth.uid())) with check (user_id = (select auth.uid()));

create policy key_envelopes_select_own on zephyr.key_envelopes for select to authenticated
  using (user_id = (select auth.uid()));
create policy key_envelopes_insert_own on zephyr.key_envelopes for insert to authenticated
  with check (
    user_id = (select auth.uid())
    and (recipient_device_id is null or exists (
      select 1 from zephyr.devices d
       where d.id = recipient_device_id and d.user_id = (select auth.uid()) and d.revoked_at is null))
  );

create policy records_select_own on zephyr.records for select to authenticated
  using (user_id = (select auth.uid()));
create policy records_insert_own on zephyr.records for insert to authenticated
  with check (user_id = (select auth.uid()));
create policy records_update_own on zephyr.records for update to authenticated
  using (user_id = (select auth.uid())) with check (user_id = (select auth.uid()));

-- Grants mirror the policies. Signed-in users only; nothing for anon.
grant usage on schema zephyr to authenticated, service_role;
grant select, insert on zephyr.devices to authenticated;
grant update (display_name, last_seen_at, revoked_at) on zephyr.devices to authenticated;
grant select, insert on zephyr.key_envelopes to authenticated;
grant select, insert on zephyr.records to authenticated;
grant update (key_gen, sealed, deleted, device_id) on zephyr.records to authenticated;
grant usage on sequence zephyr.records_seq to authenticated;
grant execute on function zephyr.push_record(text, uuid, bigint, int, bytea, boolean, uuid) to authenticated;
revoke execute on function zephyr.push_record(text, uuid, bigint, int, bytea, boolean, uuid) from anon, public;
revoke all on schema zephyr_private from public, anon, authenticated;
grant all on all tables in schema zephyr to service_role;

-- Realtime: new devices (approval prompts) and record changes (pull now).
alter publication supabase_realtime add table zephyr.devices, zephyr.records;

-- personal-apps uses an explicit PostgREST schema allow-list; keep the existing ones and add zephyr.
alter role authenticator set pgrst.db_schemas = 'residency_map, murmur, relay, worth, osler, zephyr';
notify pgrst, 'reload config';
notify pgrst, 'reload schema';
