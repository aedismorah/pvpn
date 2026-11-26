# Telegram Desktop через pvpn

## Почему файлы шли не через pvpn

Telegram Desktop **не использует системный прокси Windows/PAC** для своей сети.
У него отдельная настройка прокси внутри клиента. Поэтому `Enable system proxy`
в `pvpn-desktop` влияет на браузер и WinINET-приложения, но не на Telegram.

Старый профиль Telegram на этой машине был настроен на `127.0.0.1:10808`
(прежний `xray.exe`), а не на pvpn.

## Настройка

1. Запустить `dist\windows\pvpn-desktop.exe`.
2. Убедиться, что ingress и PSK заполнены.
3. Нажать **Start tunnel**. Нужен именно туннель, а не только system proxy:
   SOCKS5 слушает `127.0.0.1:11080`.
4. В Telegram Desktop:
   **Settings → Advanced → Connection type → Use custom proxy → Add proxy**.
5. Заполнить:

   ```text
   Type:     SOCKS5
   Hostname: 127.0.0.1
   Port:     11080
   Secret:   (пусто)
   ```

6. Сохранить и выбрать этот прокси.
7. Удалить/отключить старый прокси `127.0.0.1:10808`, если он остался.
8. Полностью перезапустить Telegram, чтобы он закрыл старые TCP-соединения
   через прежний прокси.

В списке прокси запись должна стать **online**:

![Telegram proxy online](telegram-proxy-online.png)

## Как проверить, что файлы пойдут через pvpn

Пока Telegram работает через наш SOCKS5, все его TCP-соединения должны идти на
локальный порт `11080`:

```powershell
$tg = Get-Process Telegram | Select-Object -First 1
Get-NetTCPConnection -OwningProcess $tg.Id -State Established |
  Group-Object RemoteAddress,RemotePort |
  Sort-Object Count -Descending
```

Ожидаемо:

```text
Count Name
----- ----
  N   127.0.0.1, 11080
```

Если видны соединения на реальные IP Telegram (например, `149.154.x.x:443`),
значит клиент всё ещё использует старый прокси или прямой маршрут.

Файлы в Telegram загружаются по MTProto в дата-центр через тот же прокси, что и
сообщения; отдельного «CDN-обхода» для загрузки нет. Поэтому после переключения
прокси на `127.0.0.1:11080` загрузка файлов идёт через
Cloud.ru → VDSina.

## Если загрузка всё равно не идёт

- Посмотреть, что `127.0.0.1:11080` слушает:
  ```powershell
  Get-NetTCPConnection -LocalPort 11080 -State Listen
  ```
- Проверить, что `pvpn-client`/UI не остановлен и в логах нет ошибок.
- Удалить старый прокси `10808` из Telegram и перезапустить клиент.
- Большие файлы могут упираться в скорость relay; в логах Cloud.ru видно
  выбранный exit.
- Голосовые/видеозвонки Telegram используют UDP и через текущий pvpn
  (TCP-only SOCKS5) не пойдут; это отдельная задача UDP/QUIC.
