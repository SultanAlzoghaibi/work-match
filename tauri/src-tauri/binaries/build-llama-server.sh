#!/bin/sh
# Builds a self-contained llama-server (static, Metal shaders embedded) and
# copies it to llama-server-<target-triple>, the name Tauri sidecars need.
# The official macOS release links libllama/libggml dylibs via @loader_path,
# which a single-file sidecar can't carry. Needs Xcode command line tools,
# cmake and git.
set -e
cd "$(dirname "$0")"
tag="${LLAMA_TAG:-b11669}"
# Real path: macOS's TMPDIR (/var/folders/...) is a symlink to /private/var,
# and llama.cpp's UI asset step breaks when the two spellings mix.
tmp="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
src="$tmp/llama.cpp-$tag"
[ -d "$src" ] || git clone --depth 1 --branch "$tag" https://github.com/ggml-org/llama.cpp "$src"
cmake -S "$src" -B "$src/build" -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF -DGGML_METAL_EMBED_LIBRARY=ON \
  -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
cmake --build "$src/build" --config Release --target llama-server -j
triple="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
triple="${triple:-$(uname -m | sed 's/arm64/aarch64/')-apple-darwin}"
# rm first: cp onto build.rs's empty placeholder would keep its non-executable
# mode, and the app then can't start the sidecar (every summary falls back).
rm -f "llama-server-$triple"
cp "$src/build/bin/llama-server" "llama-server-$triple"
chmod +x "llama-server-$triple"
echo "built binaries/llama-server-$triple"
# Should list only /usr/lib and /System libraries, no libllama or libggml.
otool -L "llama-server-$triple"
