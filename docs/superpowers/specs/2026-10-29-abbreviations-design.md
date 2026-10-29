# Từ điển viết tắt

## Mục tiêu

Gõ tắt được (`ko` → không, `ddc` → được) mà vẫn bật kiểm tra chính tả.
Cùng cơ chế này cũng sửa được từ tiếng Anh bị thêm dấu: thêm `test=test`
thì `test` không còn thành "tét".

## Hành vi

- Chỉ mở rộng ở chế độ V. Ở chế độ E, ezkey không xử lý phím nên `ko` vẫn
  là `ko`.
- Tra theo **các phím đã gõ** của từ vừa xong, không theo chữ trên màn hình.
  Vì vậy kết quả giống nhau dù kiểm tra chính tả bật hay tắt
  (`ddc` hiện "ddc" hoặc "đc", đều tra khoá `ddc`).
- Mở rộng khi gõ phím kết thúc từ: dấu cách, Enter, Tab, dấu câu. Phím đó
  vẫn được gõ ra sau phần mở rộng: `ko,` → "không,".
- Khoá không phân biệt hoa thường. Nếu chữ đầu gõ hoa thì viết hoa chữ đầu
  của kết quả: `Ko` → "Không". Viết hoa toàn bộ không xử lý riêng.
- Không mở rộng khi từ đã bị sửa bằng Backspace (engine không còn biết phím
  đã gõ), để tránh mở rộng nhầm.
- Không có "huỷ mở rộng". Muốn giữ nguyên thì xoá dòng đó khỏi từ điển hoặc
  chuyển sang E.

## File từ điển

Text UTF-8, mỗi dòng `khoá=giá trị`. Dòng trống và dòng bắt đầu bằng `#`
bị bỏ qua. Khoá là chữ cái ASCII (và chữ số, cho VNI). Giá trị có thể có
dấu cách và dấu tiếng Việt, tối đa 200 ký tự.

```
# ezkey: tu dien viet tat
ko=không
ddc=được
vn=Việt Nam
test=test
```

- macOS: `~/Library/Application Support/ezkey/abbreviations.txt`
- Windows: `%APPDATA%\ezkey\abbreviations.txt`

Menu có mục **Sửa từ điển viết tắt…**: tạo file mẫu nếu chưa có, rồi mở
bằng trình soạn thảo mặc định. ezkey đọc lại file khi thời gian sửa file
thay đổi (kiểm tra lúc kết thúc từ), nên không cần khởi động lại.

## Cách làm

### Core (`core/src/engine.rs`)

- `Engine` thêm `abbreviations: HashMap<String, String>`.
- `set_abbreviations(text: &str)`: phân tích nội dung file như trên; dòng
  sai định dạng bị bỏ qua. Đặt trong core để hai nền tảng dùng chung và test
  được.
- `process_key` với phím không phải chữ: trước khi `reset()`, nếu `raw` có
  giá trị và có trong bảng, trả về
  `Replace { backspaces: displayed.len(), text: giá trị + phím }`.
  Nếu không có thì giữ như cũ (`PassThrough`).

### FFI / macOS

- `ezkey_engine_set_abbreviations(engine, const char *utf8)`.
- `main.swift` đã gửi dấu cách và dấu câu vào engine, chỉ cần:
  - tăng bộ đệm `out` từ 64 lên 256 (giá trị dài hơn bộ đệm sẽ bị bỏ qua
    lặng lẽ);
  - đọc file lúc khởi động và khi mtime đổi;
  - thêm mục menu.
- Cần kiểm tra Enter/Tab: chúng có tới engine dưới dạng `\r`/`\t` không, và
  việc gắn ký tự cuối vào phím thật có đúng với Enter không.

### Windows

- `on_key` hiện chỉ gửi chữ cái và chữ số, các phím khác thì `reset()`.
  Cần gửi thêm Space, Enter, Tab và dấu câu (lấy ký tự qua `ToUnicode`) vào
  `process_key`.
- Đọc file và thêm mục menu giống macOS.

## Test

- Core, trong `engine.rs`: `ko ` → "không ", `Ko,` → "Không,",
  `ddc ` → "được " khi bật lẫn tắt kiểm tra chính tả, `test ` → "test ",
  `kox ` không mở rộng, `ko<o ` (có Backspace) không mở rộng, phân tích
  file (comment, dòng trống, dòng sai).
- macOS: thêm một trường hợp `ko ` vào `macos/e2e.swift`.

## Thứ tự

1. Core + test (khoảng 1 giờ).
2. macOS (khoảng 1 giờ).
3. Windows (khoảng 1 giờ, có thể làm sau).
