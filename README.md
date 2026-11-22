# TorrentService

Веб-клиент торрентов на Rust (librqbit) с веб-компонентами на фронте.
Мультипользовательский: изоляция закачек, дедуп одного торрента между
пользователями, refcount-удаление. Совместим с qBittorrent API v2 —
radarr/sonarr/lidarr подключаются как к qBittorrent.

## Возможности

- magnet + .torrent, пауза/resume, прогресс/скорости/ETA/пиры/ратио
- категории с вложенностью через `/` (как в qBittorrent): `movies/action`
- дедуп: два пользователя добавили один торрент — качается один экземпляр
- удаление = отписка; файлы сносятся, когда ушёл последний подписчик
- файловый браузер по `downloads/` с удалением (path-traversal закрыт)
- auth: `local` (users из env, argon2) или `ad` (LDAP bind против Samba AD)
- админка в вебе: создание/удаление пользователей, смена паролей, роли
  (`LOCAL_USERS` сидирует при первом старте, первый — админ; в AD-режиме
  доменные админы через `ADMINS`)
- Kerberos SSO (SPNEGO): доменные машины входят без пароля (feature `sso` + keytab)
- per-user пауза: движок встаёт, только когда паузу хотят все подписчики
- сид-лимиты: по ratio и/или времени (`SEED_RATIO_LIMIT`, `SEED_TIME_MINUTES`)
- смена категории перемещает файлы на диске (для завершённых торрентов)
- Swagger UI: `/swagger-ui`
- qBittorrent API v2 подмножество: `/api/v2/*` — для *arr-стека

## Запуск (dev)

```sh
cp .env.example .env          # задать SECRET (openssl rand -hex 32)
docker compose up --build     # http://localhost:8280
```

## Интеграция с home_server

1. Скопировать репо в `home_server/services/torrent_service`
2. Взять блок сервиса из `compose.home.yaml` в корневой `docker-compose.yml`
3. В `.env` домашнего сервера: `SECRET`, `TORRENT_HTTP_PORT=8280`, `TORRENT_BT_PORT=6882`
4. `AUTH_MODE=ad` — логин доменными учётками против `service_samba_addc`
5. В radarr/sonarr/lidarr: Download Client → qBittorrent,
   host `service_torrent_service`, port 8280, логин сервисной учётки

### Kerberos SSO (опционально)

На DC один раз:
```sh
samba-tool spn add HTTP/torrentsvc torrentsvc
samba-tool domain exportkeytab ./folders/torrent_service/keytab/torrent.keytab --principal=HTTP/torrentsvc
```
В `.env`: `SSO_ENABLED=true`. Доменная машина в сети с тикетом входит в веб
без пароля (браузер сам ответит на `WWW-Authenticate: Negotiate`); остальные
логинятся паролем как раньше. krb5.conf генерится автоматически в `/state`.

jellyfin/bazarr не требуют настройки — пути `/downloads` и `/media` общие.

## Конфиг (env)

| Переменная | По умолчанию | Описание |
|---|---|---|
| PORT | 8280 | HTTP |
| BT_PORT | 6882 | BitTorrent listener |
| DOWNLOADS_DIR | ./downloads | куда качается |
| PERSIST_DIR | ./state | state.json + сессия librqbit |
| AUTH_MODE | local | local \| ad |
| LOCAL_USERS | — | user:pass,... для local (первый = админ; сидируется при пустой БД) |
| ADMINS | — | доменные логины-админы для AUTH_MODE=ad |
| AD_URL | — | ldap://service_samba_addc:389 для ad |
| AD_DOMAIN | HOME.ARPA | realm для bind user@DOMAIN |
| SECRET | генерится | HMAC-ключ сессий (hex) |
| SSO_ENABLED | false | SPNEGO/Kerberos вход (сборка с `--features sso`) |
| SSO_SPN | — | HTTP/host@REALM (пусто = default keytab principal) |
| SEED_RATIO_LIMIT | 0 | сидировать до ratio N (0 = без лимита) |
| SEED_TIME_MINUTES | 0 | сидировать не дольше N минут (0 = без лимита) |

## Тесты

```sh
cargo test
```
