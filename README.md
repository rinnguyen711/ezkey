# ezkey

Bộ gõ tiếng Việt (Telex và VNI) cho Windows và macOS. Lõi viết bằng Rust, mỗi hệ điều hành có một lớp vỏ mỏng.

```
core/      Lõi Telex/VNI (Rust, không phụ thuộc hệ điều hành) + C FFI
windows/   App khay hệ thống cho Windows (Rust, low-level keyboard hook)
macos/     App menu bar cho macOS (Swift, CGEventTap, gọi lõi qua FFI)
```

## Cách gõ

### Telex

| Phím | Kết quả | Ví dụ |
|---|---|---|
| `s f r x j` | sắc, huyền, hỏi, ngã, nặng | `tieengs` → tiếng |
| `z` | xoá dấu thanh | `asz` → a |
| `aa ee oo` | â ê ô | `vieetj` → việt |
| `aw ow uw`, `w` | ă ơ ư, ư | `dduwowngf` → đường, `tw` → tư |
| `dd` | đ | `ddi` → đi |

- Bỏ dấu sau cũng được: `tienge` → tiêng, `tuongw` → tương.
- Gõ lặp phím để huỷ: `ass` → as, `aaa` → aa, `ww` → w.
- Từ không phải tiếng Việt sẽ được trả lại nguyên phím đã gõ: `world`, `windows`, `class` giữ nguyên.

### VNI

| Phím | Kết quả | Ví dụ |
|---|---|---|
| `1 2 3 4 5` | sắc, huyền, hỏi, ngã, nặng | `tie6ng1` → tiếng |
| `0` | xoá dấu thanh | `a10` → a |
| `6` | â ê ô | `vie65t` → việt |
| `7`, `8` | ơ ư, ă | `d9u7o7ng2` → đường, `a8n` → ăn |
| `9` | đ | `d9i` → đi |

Gõ lặp số để huỷ: `a11` → a1. Số đứng một mình vẫn là số: `123` giữ nguyên.

### Chung

- Có hai kiểu đặt dấu: cũ (hòa, thúy, mặc định) và mới (hoà, thuý).
- **Ctrl+Shift** bật/tắt tiếng Việt. Trên Windows, vào menu **Phím tắt bật/tắt** để:
  - chọn **Alt+Z**, hoặc tắt hẳn phím tắt;
  - chọn **Đặt phím tắt khác…** rồi nhấn tổ hợp bạn muốn, ví dụ Ctrl+Alt+V, Ctrl+Space, Ctrl+Alt hoặc F9. Nhấn Esc để huỷ. Nếu không nhấn gì trong 10 giây, ezkey tự huỷ.
  - Phím tắt phải có Ctrl, Alt hoặc Win, hoặc là một phím đặc biệt như F1–F24, Pause, Scroll Lock, Caps Lock. Một chữ cái đứng riêng hay Shift+chữ sẽ bị từ chối, vì đó là phím để gõ chữ.
- **Nhớ chế độ V/E theo từng app** (Windows, bật sẵn): mỗi app giữ chế độ bạn chọn lần cuối trong app đó. Ví dụ để VS Code ở E và Zalo ở V; khi chuyển qua lại giữa hai app, ezkey tự đổi theo. Danh sách được lưu trong registry, tại `HKCU\Software\ezkey\Apps`.
- Kiểu gõ, kiểu đặt dấu và trạng thái bật/tắt được đổi trong menu và được nhớ cho lần mở sau.

## Windows

```
cargo build --release -p ezkey-win
target\release\ezkey.exe
```

Biểu tượng **V** (đang gõ tiếng Việt) hoặc **E** (English) hiện ở khay hệ thống. Bấm chuột trái để bật/tắt, chuột phải để mở menu: Telex/VNI, kiểu đặt dấu, **Khởi động cùng Windows**.

Cài đặt được lưu trong registry, tại `HKCU\Software\ezkey`.

Không chạy cùng lúc với Unikey hoặc EVKey: hai bộ gõ sẽ cùng xử lý phím. Nếu bắt buộc phải chạy cả hai, hãy đổi phím tắt của ezkey sang Alt+Z để hai bên không cùng bật/tắt theo Ctrl+Shift.

Muốn gõ được trong các app chạy quyền Administrator thì ezkey cũng phải chạy quyền Administrator. Bật mục **Chạy với quyền Administrator** trong menu:

- ezkey khởi động lại với quyền admin. Windows sẽ hiện hộp thoại UAC để xác nhận. Từ đó về sau, mỗi lần mở ezkey đều xin quyền admin.
- Nếu bật thêm **Khởi động cùng Windows**, ezkey dùng Task Scheduler (chế độ "highest privileges") thay cho registry Run. Nhờ vậy ezkey chạy quyền admin ngay khi đăng nhập mà không hiện UAC.
- Tắt mục này thì từ lần mở sau ezkey chạy lại với quyền thường.
- Khi ezkey đang chạy quyền admin, chú thích của biểu tượng ở khay có chữ `[Admin]`.

## macOS

Cần Xcode Command Line Tools và Rust.

```
sh macos/build.sh
open macos/build/ezkey.app
```

Lần đầu chạy, cấp quyền tại System Settings → Privacy & Security → Accessibility. Menu có mục **Mở cùng máy** (cần macOS 13 trở lên).

Bản macOS chưa được build hay chạy thử trên máy Mac thật.

## Kiểm thử

```
cargo test -p ezkey-core                      # lõi Telex/VNI
cargo build --release -p ezkey-win
cargo run --release -p ezkey-win --example e2e -- target\release\ezkey.exe
```

Bài test `e2e` mở Notepad, gõ phím thật qua `SendInput`, rồi đọc lại văn bản. Cần tắt các bộ gõ tiếng Việt khác trước khi chạy; bài test sẽ tự dừng nếu phát hiện có bộ gõ khác. Trong lúc test chạy (khoảng 20 giây), đừng bấm phím hay click chuột. Bài test cũng cần tắt ezkey đang chạy trước. Test sẽ tạm đặt cài đặt trong registry rồi trả lại như cũ khi xong.
