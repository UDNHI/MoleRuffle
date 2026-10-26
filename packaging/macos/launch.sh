#!/bin/sh
set -eu
resources_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
macos_dir="$resources_dir/../MacOS"
data_dir="${MOLERUFFLE_TEST_DATA_DIR:-$HOME/Library/Application Support/MoleRuffle-Taiwan-Test}"
mkdir -p "$data_dir"
export MOLERUFFLE_DATA_DIR="$data_dir"
export MOLERUFFLE_BANK_SWF="$data_dir/bank-patched.swf"
export RUST_LOG="warn,moleruffle=info,moleruffle_core=info,ruffle_core::avm2=info"
export RUST_BACKTRACE=1
export RUST_LIB_BACKTRACE=0
# Set only this game process timezone, never the macOS system timezone.
export TZ=Asia/Taipei
run_stamp=$(date +%Y%m%d-%H%M%S)
log_file="$data_dir/test-$run_stamp.log"
prepare() {
    source_file="$data_dir/bank-source.swf"
    tmp_dir=$(mktemp -d "$data_dir/prepare.XXXXXX")
    trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM
    if [ ! -f "$source_file" ]; then
        printf '首次啟動：正在下載官方銀行資源…\n'
        /usr/bin/curl --fail --location --connect-timeout 15 --max-time 60 --retry 1 \
            'http://mole.61.com.tw/resource/map/69.swf' -o "$tmp_dir/bank-source.swf" || return 1
        source_file="$tmp_dir/bank-source.swf"
    fi
    "$macos_dir/bank-patch" "$source_file" "$resources_dir/bank.patch" "$tmp_dir/bank-patched.swf" || return 1
    if [ "$source_file" != "$data_dir/bank-source.swf" ]; then
        mv "$source_file" "$data_dir/bank-source.swf"
    fi
    mv "$tmp_dir/bank-patched.swf" "$MOLERUFFLE_BANK_SWF"
    rm -rf "$tmp_dir"
    trap - EXIT HUP INT TERM
}
if ! prepare >> "$log_file" 2>&1; then
    cat "$log_file"
    exit 1
fi
# Packaging smoke test: prepare/download only, without opening a game window.
if [ "${MOLERUFFLE_PREPARE_ONLY:-0}" = 1 ]; then
    printf 'Preparation successful.\n'
    exit 0
fi
exec "$macos_dir/moleruffle" >> "$log_file" 2>&1
