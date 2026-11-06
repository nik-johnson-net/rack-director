# --- rack-director: hand first boot to cloud-init (generated) ---
# cloud-init fetches its config from @@SEED_URL@@ on first boot;
# @@UNIT_NAME@@ then reports the result back to rack-director.
(
set -e
rd_root="${RD_TARGET:-}"
install -d -m 0755 "$rd_root/etc/cloud/cloud.cfg.d" "$rd_root/usr/local/sbin" "$rd_root/etc/systemd/system"
cat > "$rd_root@@DROP_IN_PATH@@" <<'RD_EOF'
@@DROP_IN@@RD_EOF
cat > "$rd_root@@REPORTER_PATH@@" <<'RD_EOF'
@@REPORTER@@RD_EOF
chmod 0755 "$rd_root@@REPORTER_PATH@@"
cat > "$rd_root@@UNIT_PATH@@" <<'RD_EOF'
@@UNIT@@RD_EOF
if [ -n "$rd_root" ]; then
    systemctl --root="$rd_root" enable @@UNIT_NAME@@
    if [ -x "$rd_root/usr/sbin/restorecon" ]; then chroot "$rd_root" /usr/sbin/restorecon -F @@PATHS@@; fi
else
    systemctl enable @@UNIT_NAME@@
    if command -v restorecon >/dev/null 2>&1; then restorecon -F @@PATHS@@; fi
fi
)
rd_status=$?
if [ "$rd_status" -ne 0 ]; then
    echo "rack-director: cloud-init setup failed (exit $rd_status)" >&2
    exit "$rd_status"
fi
# --- end rack-director ---
