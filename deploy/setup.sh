#!/usr/bin/env bash
# Install or update an Among Azeroth lobby on an Ubuntu VPS, as root. deploy/vps.ps1 uploads this
# with the binary and files to /tmp/among-deploy and runs it; params.env there holds the settings.
set -euo pipefail

stage=/tmp/among-deploy
app=/opt/among-azeroth
envfile=/etc/among-azeroth.env

[ "$(id -u)" -eq 0 ] || { echo "setup.sh: run as root (ssh root@<vps>)" >&2; exit 1; }
# MODE (server|host), and optionally LOBBY_CODE, OWNER, DOMAIN, PUBLIC_IP.
. "$stage/params.env"

# The address the VPS reaches the internet from, its public IP on a plain VPS (nothing is sent).
PUBLIC_IP=${PUBLIC_IP:-$(ip -4 route get 1.1.1.1 | awk '{for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1)}')}
# A domain given once survives redeploys, like the lobby code.
if [ -z "${DOMAIN:-}" ] && [ -f "$envfile" ]; then
    DOMAIN=$(sed -n 's|^PUBLIC_URL=https://||p' "$envfile")
fi
# sslip.io resolves 203-0-113-7.sslip.io to 203.0.113.7, so Caddy gets a certificate with no domain.
DOMAIN=${DOMAIN:-${PUBLIC_IP//./-}.sslip.io}

id among >/dev/null 2>&1 || useradd --system --home-dir "$app" --shell /usr/sbin/nologin among
install -d -o root -g root -m 755 "$app" "$app/Data"
install -d -o among -g among -m 755 "$app/benilla-config" "$app/benilla-config/among"

# A new binary replaces the old one under the running service; the restart below picks it up.
bin=among-$MODE
install -o root -g root -m 755 "$stage/$bin" "$app/$bin.new"
mv -f "$app/$bin.new" "$app/$bin"
if [ -f "$stage/marks.toml" ]; then
    install -o among -g among -m 644 "$stage/marks.toml" "$app/benilla-config/among/marks.toml"
fi
layout_args=
if [ -f "$stage/layout.toml" ]; then
    install -o root -g root -m 644 "$stage/layout.toml" "$app/layout.toml"
    layout_args="--layout $app/layout.toml"
fi

# The lobby code survives redeploys unless a new one is given.
if [ -z "${LOBBY_CODE:-}" ] && [ -f "$envfile" ]; then
    LOBBY_CODE=$(sed -n 's/^LOBBY_CODE=//p' "$envfile")
fi
LOBBY_CODE=${LOBBY_CODE:-$(tr -dc 'A-Z0-9' </dev/urandom | head -c 6)}
owner_args=
[ -n "${OWNER:-}" ] && owner_args="--account $OWNER"
umask 077
cat >"$envfile" <<EOF
PUBLIC_IP=$PUBLIC_IP
PUBLIC_URL=https://$DOMAIN
LOBBY_CODE=$LOBBY_CODE
LAYOUT_ARGS=$layout_args
OWNER_ARGS=$owner_args
EOF
umask 022

if [ "$MODE" = host ]; then
    rm -rf "$app/www"
    install -d -m 755 "$app/www"
    tar -xzf "$stage/www.tar.gz" -C "$app/www"
    command -v caddy >/dev/null || { apt-get update -q && apt-get install -y -q caddy; }
    printf '%s {\n\treverse_proxy 127.0.0.1:8090\n}\n' "$DOMAIN" >/etc/caddy/Caddyfile
    # A real domain also answers on www., sent on to the one address the invite names.
    case $DOMAIN in
    *.sslip.io) ;;
    *) printf 'www.%s {\n\tredir https://%s{uri} permanent\n}\n' "$DOMAIN" "$DOMAIN" >>/etc/caddy/Caddyfile ;;
    esac
    systemctl enable caddy >/dev/null 2>&1
    systemctl reload-or-restart caddy
fi

if ufw status 2>/dev/null | grep -q 'Status: active'; then
    ufw allow 22/tcp >/dev/null
    ufw allow 3724/tcp >/dev/null
    ufw allow 8085/tcp >/dev/null
    if [ "$MODE" = host ]; then
        ufw allow 80/tcp >/dev/null
        ufw allow 443/tcp >/dev/null
        # Caddy serves HTTP/3 over UDP.
        ufw allow 443/udp >/dev/null
    fi
fi

# BBR keeps big downloads (the game data) fast on lossy home links; Ubuntu's kernel ships it.
cat >/etc/sysctl.d/90-among-azeroth.conf <<EOF
net.core.default_qdisc=fq
net.ipv4.tcp_congestion_control=bbr
EOF
sysctl -q -p /etc/sysctl.d/90-among-azeroth.conf || echo "setup.sh: BBR unavailable, keeping the default"

other=$([ "$MODE" = host ] && echo server || echo host)
install -m 644 "$stage/among-$MODE.service" /etc/systemd/system/
systemctl daemon-reload
systemctl disable --now "among-$other" >/dev/null 2>&1 || true
systemctl enable "among-$MODE" >/dev/null 2>&1
systemctl restart "among-$MODE"
rm -rf "$stage"

sleep 2
systemctl is-active --quiet "among-$MODE" || {
    journalctl -u "among-$MODE" -n 30 --no-pager
    exit 1
}
echo
echo "Among Azeroth is running on $PUBLIC_IP. Lobby code: $LOBBY_CODE"
case $DOMAIN in
*.sslip.io) echo "Native players: realmlist $PUBLIC_IP" ;;
*) echo "Native players: realmlist $DOMAIN" ;;
esac
if [ "$MODE" = host ]; then echo "Browser players: https://$DOMAIN/?lobby=$LOBBY_CODE"; fi
echo "Logs: journalctl -u among-$MODE -f"
