# SSH Tunnel

Клиент для Windows на Rust. Поднимает системный IPv4-туннель через SSH
на порту 22 или локальный SOCKS5-прокси. В обоих режимах можно войти
по паролю или SSH-ключу. Подключение работает в фоне.

## 1. Установить серверную часть

Нужны Debian или Ubuntu, права root, SSH на порту 22 и доступ к
`/dev/net/tun`. Скопируй [install-server.sh](install-server.sh) на сервер
и выполни из папки с файлом:

```sh
sudo sh install-server.sh
```

Установщик добавит недостающие пакеты, проверит TUN, включит IPv4 forwarding
и установит обработчик в `/usr/local/libexec/ssh-tunnel-vpn`. Обработчик
запускается внутри SSH-сеанса от root, поддерживает одно VPN-подключение
и удаляет временные правила маршрутизации и NAT при штатном завершении.
Существующие настройки входа по SSH определяют, можно ли подключаться
с выбранным способом авторизации.

Для **SOCKS5** достаточно SSH-сервера с разрешённым TCP-форвардингом.

## 2. Собрать клиент Windows x64

Установи [Rust и Cargo](https://rustup.rs/) и включи компонент
**OpenSSH Client** в Windows. Выполни в PowerShell:

```powershell
git clone https://github.com/d1llr/sshvpn.git
cd sshvpn
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
cargo +stable-x86_64-pc-windows-gnu build --release --locked
```

Исполняемый файл появится в `target\release\ssh-tunnel.exe`.
Для сборки через MSVC используй `cargo build --release --locked` с
установленными Visual Studio Build Tools для C++ и Windows SDK.

Для **System VPN (IPv4)** скачай официальный архив [Wintun](https://www.wintun.net/)
и скопируй из него `wintun\bin\amd64\wintun.dll` в `target\release`,
рядом с `ssh-tunnel.exe`.

## 3. Подключиться

1. При первом подключении выполни `ssh USER@SERVER` из-под той же учётной
   записи Windows, с которой запускаешь клиент. Сверь показанный отпечаток
   с доверенным источником и подтверди только совпадающий ключ. OpenSSH
   сохранит его в `%USERPROFILE%\.ssh\known_hosts`.
2. Для системного VPN запусти `ssh-tunnel.exe` **от имени администратора**,
   выбери **System VPN (IPv4)** и укажи сервер и SSH-пользователя root.
   Для прокси выбери **SOCKS5** и укажи пользователя с доступом к SSH.
3. Выбери **Password** или **SSH key** и нажми **Connect**. В режиме прокси
   укажи `127.0.0.1:1080` в настройках приложения или используй
   **Copy proxy address**, если выбрал другой локальный порт.
4. Нажми **Disconnect**, чтобы закрыть туннель. **Save profile** сохраняет
   параметры в `%LOCALAPPDATA%\SshTunnelClient\profile.json`. Пароль
   и парольная фраза в файл профиля не записываются.

## Маршрутизация

Системный VPN направляет IPv4-трафик по умолчанию через Wintun, включая UDP.
Более специфичные маршруты, например к локальной сети и SSH-серверу,
сохраняют приоритет. На адаптере используются DNS `1.1.1.1` и `1.0.0.1`.
Публичный IPv6 без более специфичного маршрута направляется в Wintun
и отбрасывается. Системный DNS может использовать другие интерфейсы.
После аварийного завершения клиента маршрут к SSH-серверу может
сохраниться до перезагрузки Windows.
