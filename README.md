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

jellyfin/bazarr не требуют настройки — пути `/downloads` и `/media` общие.

## Конфиг (env)

| Переменная | По умолчанию | Описание |
|---|---|---|
| PORT | 8280 | HTTP |
| BT_PORT | 6882 | BitTorrent listener |
| DOWNLOADS_DIR | ./downloads | куда качается |
| PERSIST_DIR | ./state | state.json + сессия librqbit |
| AUTH_MODE | local | local \| ad |
| LOCAL_USERS | — | user:pass,... для local |
| AD_URL | — | ldap://service_samba_addc:389 для ad |
| AD_DOMAIN | HOME.ARPA | realm для bind user@DOMAIN |
| SECRET | генерится | HMAC-ключ сессий (hex) |

## Тесты

```sh
cargo test
```
