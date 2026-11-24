# ConvertAnythings v1 cho Windows

## Mục tiêu

Ứng dụng Windows x64 xử lý tệp cục bộ, chạy trong khay hệ thống và ẩn khỏi taskbar. Kéo một tệp từ File Explorer, bấm F8 để mở menu vòng tròn gần con trỏ, rê tới định dạng và thả để chuyển đổi. Người dùng có thể đổi phím tắt, mở vùng thả trước khi kéo hoặc thoát từ khay.

## Thiết kế

- Rust với `windows-rs`. Win32 đăng ký phím tắt và chờ thông điệp. Overlay Win32 trong suốt dùng Direct2D/DirectWrite, đặt trong vùng làm việc của màn hình hiện tại và tính kích thước theo DPI. Tài nguyên menu được giải phóng khi đóng.
- OLE `IDropTarget` nhận `CF_HDROP`. Chỉ nhận một tệp hệ thống mỗi lượt. Menu hiện các định dạng phù hợp, có nhóm con khi có nhiều nhóm đích. Trả hiệu ứng copy khi chấp nhận tác vụ để Explorer giữ tệp gốc. Escape hủy lượt kéo.
- Một tiến trình con chạy từng tác vụ. CLI `--convert --to <format> <path>` dùng cùng mã chuyển đổi. Kết quả được tạo tạm trong cùng thư mục rồi công bố sau khi thành công, dùng hậu tố `(1)`, `(2)` khi trùng tên.

| Đầu vào | Đầu ra | Bộ chuyển đổi |
|---|---|---|
| JPG, PNG, BMP, TIFF, WebP tĩnh | Các định dạng khác trong nhóm | WIC và FFmpeg cho WebP |
| PDF | PNG, JPG | Windows.Data.Pdf, từng trang ở 150 DPI, PDF nhiều trang xuất thư mục `<tên>-pages` |
| TXT, DOCX | PDF | Word Desktop qua COM, TXT UTF-8 hoặc UTF-16 có BOM, mở chỉ đọc và tắt macro |
| MP4, MOV, MKV, WebM | MP4 H.264/AAC, WebM VP9/Opus, MP3 | FFmpeg, MP3 lấy luồng âm thanh đầu tiên |
| MP3, WAV, FLAC, M4A, OGG | MP3, WAV, FLAC | FFmpeg |

## Mốc triển khai

1. Khởi tạo Git cục bộ và commit spec.
2. Kiểm chứng khay hệ thống, F8 và kéo thả từ Explorer.
3. Hoàn thiện menu vòng tròn và chuyển ảnh.
4. Thêm PDF và xuất PDF từ Word.
5. Thêm âm thanh và video.
6. Chạy kiểm thử trên Windows, build Release và ghi nhận mức tài nguyên khi chờ. Đo RAM trước khi chốt ngưỡng. Commit sau mỗi mốc đã kiểm tra.

## Tham khảo Klick

[Klick](https://github.com/imggion/klick) mô tả hai cách kích hoạt trên macOS. Lượt giữ Shift có thể đọc mục đang chọn trong Finder qua Apple Events, còn chế độ kéo nhận tệp khi người dùng thả lên menu. V1 Windows tập trung vào chế độ kéo thả bằng OLE và dùng F8 mặc định. Tham khảo cách trình bày danh sách tác vụ theo loại tệp và cách ghi kết quả cạnh tệp gốc. Giao diện Windows được vẽ riêng.

## Nhật ký triển khai và khác biệt trên Windows

- Đã xác nhận bản Release nhận được `CF_HDROP` từ Explorer thật khi bấm F8 sau khi đã bắt đầu kéo một BMP. Sau khi di chuyển con trỏ vào cửa sổ mới, Explorer gọi `DragEnter`, rồi `Drop` khi nhả chuột. Tệp gốc vẫn nằm ở vị trí cũ. Đây là hai sự kiện riêng, vì phím tắt không mang đường dẫn và cửa sổ phải trở thành đích của lượt kéo.
- Menu Direct2D đã được kiểm tra bằng ảnh chụp từ phiên Windows và bằng thao tác chọn ô JPG trong Explorer. Bộ kiểm thử ảnh xác nhận các container JPG/PNG/BMP/TIFF/WebP, nền trắng cho JPG, giữ alpha khi chuyển qua TIFF/WebP, tên Unicode, trùng tên và dọn kết quả tạm khi lỗi.
- `Windows.Data.Pdf` áp tỷ lệ màn hình khi render. Tại tỷ lệ 125%, trang mẫu yêu cầu 300 × 150 pixel trả về 375 × 188 pixel. WIC chuẩn hóa ảnh theo kích thước vật lý và ghi 150 DPI. Đã kiểm tra trang đơn, nhiều trang và PDF hỏng.
- Word COM cần xử lý cách truyền tham số khi đóng phiên và không có HWND cho tài liệu mở ẩn. Bộ xuất dùng một phiên Word riêng, đưa TXT đã chuẩn hóa Unicode vào tài liệu mới và mở DOCX chỉ đọc. Kiểm thử xác nhận TXT UTF-8/UTF-16 giữ chữ tiếng Việt, DOCX giữ khổ Letter và hai ô bảng, và tài liệu Word khác đang mở không bị đóng hoặc sửa.
- `ffprobe` xác nhận H.264/AAC, VP9/Opus, MP3, PCM 16-bit và FLAC. Bài thử cũng kiểm tra video im lặng, đầu vào lỗi và khả năng báo thiếu FFmpeg. MP3 từ video im lặng báo lỗi mà không tạo tệp đầu ra.
- Git trên Windows ban đầu từ chối repository do tài khoản sandbox tạo thư mục `.git`. Đã kiểm tra bản sao bằng `git fsck`, chuyển metadata về thư mục thuộc tài khoản người dùng và xác nhận HEAD giữ nguyên. Thiết lập Git toàn cục giữ nguyên.

## Cách chạy bản Release

Build bằng `cargo build --locked --release`. Mở `target\release\convert-anythings.exe` để chương trình chờ trong khay hệ thống. Kéo một tệp, bấm F8, rê vào lựa chọn và thả. Có thể đổi sang F9 hoặc Ctrl+Alt+C, hoặc mở vùng thả từ menu khay. Chọn **Thoát** ở khay để kết thúc tiến trình. Nếu một lượt chuyển đang xử lý, ứng dụng chờ kết quả rồi mới thoát.

Ảnh chuẩn và PDF dùng thành phần Windows. WebP và video/âm thanh cần một FFmpeg trên PATH có các encoder tương ứng. TXT/DOCX sang PDF cần Word Desktop có thể chạy. EXE dùng các bộ chuyển đổi đang có trên máy. Tệp PDF nhiều trang được lưu nguyên nhóm trong thư mục kết quả cạnh nguồn.

## Phạm vi khác so với bản demo macOS và Klick

| Tính năng | Bản Windows v1 |
|---|---|
| Chọn tệp sẵn trong Finder rồi giữ Shift | Luồng Windows dùng tệp từ lượt kéo OLE và phím tắt F8 toàn hệ thống. Thao tác chỉ chọn tệp trong Explorer rồi giữ Shift vẫn ngoài phạm vi. |
| Nhiều tệp hoặc cả thư mục | V1 nhận một tệp trong mỗi lượt. |
| Liquid Glass và phản hồi Force Touch | Menu Windows dùng đồ họa Direct2D trong suốt với màu riêng. Hiệu ứng haptic của trackpad Apple thuộc bản macOS. |
| PDF sang DOCX, OCR, cắt ảnh, thêm nền | V1 chỉ có các cặp chuyển đổi trong bảng trên. PDF sang DOCX, OCR và các công cụ chỉnh sửa trong demo cần mốc riêng. |
| Tài liệu có tài nguyên được liên kết từ xa | DOCX có liên kết ảnh, mẫu hoặc dữ liệu bên ngoài cần nhúng tài nguyên trước khi ứng dụng cho phép xuất offline. |

## Nghiệm thu Windows x64

- `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo test --locked --offline` và `cargo build --locked --offline --release` đều chạy thành công. Ba unit test kiểm tra lựa chọn đích và cách đi từ nhóm sang ô định dạng. `scripts/conversion-smoke.ps1` kiểm tra đầu ra ảnh, PDF, Word và media bằng tệp mẫu, métadữ liệu ảnh, nội dung PDF và `ffprobe`.
- `scripts/explorer-smoke.ps1` đã chạy qua BMP → JPG, PDF → JPG, DOCX → PDF, WAV → MP3 và MP4 → WebM trên Explorer thật. Bài thử MP4 đi qua nhóm Video trước khi chọn WebM. Một lượt riêng kiểm tra F9 và Esc hủy khi đang kéo. Ban đầu một số lượt mô phỏng mở được menu mà chưa bắt đầu kéo OLE. Đưa cửa sổ Explorer thử nghiệm lên trước và chọn đúng tệp bằng Shell trước khi giữ chuột đã cho phép kiểm chứng trọn luồng.
- Bản Release nặng 587.776 byte. `scripts/idle-smoke.ps1` đo hai khoảng chờ 30 giây, trước và sau 20 lượt mở–đóng menu. RAM riêng đang nằm trong bộ nhớ vật lý lần lượt là **1,07 MiB** và **3,47 MiB**. Working set gồm cả trang dùng chung là **7,95 MiB** và **15,30 MiB**. CPU ghi **0,0000%** ở độ phân giải phép đo trong cả hai khoảng. Bộ đếm GPU hoạt động nhưng không có mẫu dành cho PID của ứng dụng. Không có lần vẽ menu khi chờ.
- Trên hai màn hình 120 và 96 DPI, menu nằm trong vùng làm việc ở cả 20 lượt thử gần góc màn hình. Từ lúc ứng dụng nhận thông điệp F8 đến khung đầu tiên, lần đầu mất **43,64 ms**, trung vị **30,30 ms**, p95 **41,96 ms**. Các số này đo trên máy hiện tại và dùng menu chờ tệp, chưa bao gồm thời gian Windows chuyển phím tới tiến trình hay thời gian chuyển đổi tệp.

## ConvertAnythings v2

### Giao diện, Logo và Trải nghiệm người dùng
- **Logo & Icon ứng dụng**: Biểu tượng `assets/app.ico` (16/24/32/48/256 px) được nhúng trực tiếp vào tài nguyên Win32 của file `.exe` qua `build.rs` (hiển thị trong File Explorer, taskbar/khay hệ thống và hộp thoại), không cần thêm crate ngoài.
- **Giao diện Tangerine Glass & Obsidian Glass (`src/render.rs`, `src/menu.rs`)**:
  - Đổi từ bảng màu teal cũ sang phong cách **Tangerine Glass** (mặc định: nền kính mờ ấm, cánh hoa bo góc, gradient cam san hô `#FF763F` → `#E64415` khi rê vào, tâm vòng hiển thị tên đích + phụ đề định dạng/số lượng tệp) và hỗ trợ chuyển sang **Obsidian Glass (Tối)** từ khay hệ thống.
  - Bố cục vòng đơn trực tiếp khi có ≤ 8 định dạng đích (thay vì bắt buộc qua 2 vòng), chỉ chuyển sang 2 vòng khi > 8 định dạng.
  - Hiệu ứng mở pop-in 115 ms (cubic ease-out) qua `ANIM_TIMER` 16 ms; dừng timer ngay khi hoàn tất nên CPU/GPU khi chờ bằng 0.
- **Thao tác mượt & Tiện ích khay hệ thống (`src/ui.rs`)**:
  - Hỗ trợ **Kéo + Shift** (`WH_KEYBOARD_LL` nhẹ, bật mặc định, bật/tắt ở khay hệ thống) bên cạnh phím tắt toàn cục (`F8`, `F9`, `Ctrl+Alt+C`), kèm phím **Esc** đóng menu nhanh.
  - Hỗ trợ **kéo thả nhiều tệp cùng lúc**: tự lọc các đích chung; nếu toàn bộ là ảnh và chọn `PDF` sẽ gộp thành một tệp PDF nhiều trang.
  - Lưu sẵn cấu hình bộ chuyển đổi khi khởi động, chạy tối đa 4 tác vụ song song không chặn nhau, thêm mục **Mở vị trí tệp vừa chuyển**, **Giao diện**, **Mở khi kéo + Shift**, **Khởi động cùng Windows** (`HKCU\...\Run`), và **Quét lại bộ chuyển đổi** trong menu khay.

### Mở rộng định dạng và tăng tốc chuyển đổi
| Đầu vào | Đầu ra | Bộ chuyển đổi & Tối ưu v2 |
|---|---|---|
| JPG, PNG, BMP, TIFF, ICO, WebP, AVIF, HEIC | JPG, PNG, WebP, AVIF, ICO, BMP, TIFF, PDF | WIC (bỏ qua bước trộn nền trắng nếu ảnh nguồn đã không có kênh alpha), FFmpeg truyền thẳng một lượt cho WebP/AVIF (`libsvtav1` `avif=1` / fallback `libaom-av1`)/ICO, và `img2pdf` thuần Rust gộp 1 hoặc nhiều ảnh thành PDF (`DCTDecode`, nhúng trực tiếp JPEG baseline không nén lại). |
| GIF | MP4, WebM, WebP, PNG, JPG, PDF | FFmpeg cho video/WebP và `img2pdf`/WIC cho ảnh/PDF. |
| PDF | PNG, JPG, WebP, TXT | `Windows.Data.Pdf` (150 DPI, nhiều trang vào `<tên>-pages`), FFmpeg cho WebP, `pdftotext` (`-enc UTF-8`) cho TXT. |
| DOCX, DOC, RTF, TXT; XLSX, XLS, CSV; PPTX, PPT | PDF | Word / Excel / PowerPoint Desktop qua COM phiên tách biệt (tắt cảnh báo & macro, giữ nguyên tài liệu đang mở). |
| MP4, MOV, MKV, WebM, AVI, WMV, FLV, M4V, TS | MP4, WebM, MOV, GIF, MP3, WAV, M4A | FFmpeg + `ffprobe`: tự động remux (`-c:v copy -c:a copy`) khi codec đã tương thích MP4/MOV/M4A; ưu tiên encoder GPU (`h264_nvenc` → `h264_qsv` → `h264_amf`) với fallback an toàn sang `libx264 -preset veryfast`; WebM dùng `libvpx-vp9` realtime row-mt; GIF dùng palette 2-pass trong một filtergraph. |
| MP3, WAV, M4A, FLAC, OGG, AAC, WMA, OPUS | MP3, WAV, M4A, FLAC, OGG | FFmpeg (`libmp3lame`, `pcm_s16le`, `aac`, `flac`, `libopus`). |

### Nghiệm thu v2 trên Windows x64
- `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo test --locked --offline` (4 unit tests), `cargo build --locked --offline --release`, và `scripts/conversion-smoke.ps1` (kiểm thử ảnh gồm WebP/AVIF/ICO/PDF/gộp nhiều ảnh, PDF raster + `PDF → TXT`, Word → PDF, và media gồm `MKV → MP4` remux, `MP4 → GIF`, `WAV → M4A/OGG`) đều đạt.
- Bản Release nặng **727.552 byte** (đã nhúng icon đa kích thước). Đo nhàn rỗi 2 khoảng 8 giây trước và sau 10 chu kỳ mở–đóng menu (trên phiên desktop không tương tác dùng `PostMessage` `OPEN_OVERLAY`):
  - Private Working Set: **0,96 MiB** (trước) và **3,64 MiB** (sau)
  - Working Set: **6,94 MiB** (trước) và **14,92 MiB** (sau)
  - CPU: **0%** trong cả hai khoảng; GPU samples cho PID: **0**; số lần vẽ khi chờ: **0**
  - Độ trễ khung đầu tiên: **55,99 ms** (lần đầu), trung vị **29,81 ms**, p95 **55,99 ms**.

## ConvertAnythings v3 (Học hỏi từ `newtonstudios/convert`)

### Các tính năng mới đã tích hợp
- **Nhận diện Magic-Byte (`src/convert/sniff.rs`)**:
  - Đọc tối đa 512 byte đầu tệp để phân biệt `.ts` (chỉ nhận MPEG-TS video khi có sync byte `0x47` chu kỳ 188/192 byte; bỏ qua file mã nguồn TypeScript) và chặn tệp đổi sai đuôi (ví dụ ảnh PNG đặt tên `.mp4`) với thông báo rõ định dạng thật vs đuôi tệp.
- **Tìm công cụ động & Cache GPU Encoder (`src/convert.rs`, `src/convert/media.rs`)**:
  - `find_tool` tìm lần lượt ở `<exe_dir>\tools\`, `PATH` tiến trình, `PATH` trực tiếp từ Registry (`HKCU\Environment` và `HKLM\...\Environment`, tự mở rộng biến `%VAR%`), và `%LOCALAPPDATA%\Microsoft\WinGet\Links\`.
  - Kết quả thăm dò encoder H.264 phần cứng (`h264_nvenc` / `h264_qsv` / `h264_amf`) được lưu cache tại `<config_dir>\hw_encoder_cache.txt` theo `(đường dẫn, kích thước, mtime)` của `ffmpeg.exe`.
- **Fallback thư mục `Downloads` khi nguồn chỉ đọc (`src/convert.rs`, `src/ui.rs`)**:
  - Nếu thư mục chứa tệp gốc không cho phép ghi (ổ đĩa khóa ghi, thư mục read-only), `Staging::new_with_fallback` tự động chuyển kết quả sang `%USERPROFILE%\Downloads` (hoặc `CONVERTANYTHINGS_FALLBACK_DIR`) và thông báo kèm tên thư mục đích.
- **Markdown → HTML, DOCX & PDF (`src/convert/markdown.rs`, `src/convert/pdf2docx.rs`)**:
  - `MD → HTML`: thuần Rust (hỗ trợ tiêu đề, code block, blockquote, danh sách, đậm/nghiêng/link/ảnh, giao diện sáng/tối tự động).
  - `MD → DOCX`: thuần Rust, tự chuyển đổi Markdown sang các thẻ `w:p`, `w:rPr` (`w:b`, `w:i`, font `Consolas` cho code, v.v.) và đóng gói file `.docx` OpenXML ZIP chuẩn.
  - `MD → PDF`: in PDF bằng Edge hoặc Chrome headless (`--headless=new --print-to-pdf`), tối ưu lề in `@page { margin: 18mm; }`, tự dọn dẹp profile tạm trong `%TEMP%`; tự động fallback qua Word Desktop nếu không có trình duyệt.
  - `PDF → DOCX`: thuần Rust (trích xuất văn bản UTF-8 bằng `pdftotext`, giữ ngắt trang, tự đóng gói `word/document.xml` thành file `.docx` ZIP chuẩn không cần Word COM nên không bị treo hộp thoại PDF Reflow).
- **Bộ công cụ PDF & Nén tệp kiểu iLovePDF (`src/convert/pdf.rs`, `src/convert/image.rs`, `src/convert/media.rs`)**:
  - **Gộp PDF (`merge-pdf`)**: Ưu tiên `pdfunite.exe` hoặc Ghostscript gộp nguyên bản vector/text; fallback render WinRT PDF + JPEG thuần Rust.
  - **Tách PDF (`split-pdf`)**: Ưu tiên `pdfseparate.exe` tách thành `<tên>-001.pdf`, `<tên>-002.pdf`, ... trong thư mục `<tên>-split`; fallback WinRT PDF.
  - **Nén PDF (`compress`)**: Ưu tiên Ghostscript (`-dPDFSETTINGS=/ebook`); fallback WinRT PDF render ở 115 DPI nén JPEG tối ưu.
  - **Nén Ảnh & Video (`compress`)**: Nén JPG/PNG/WebP/AVIF/GIF/BMP/TIFF/HEIC và Video MP4/WebM (giới hạn 1080p, tối ưu bitrate âm thanh 96k, tận dụng GPU NVENC/QSV/AMF).
- **Quy tắc kéo thả nhiều tệp thông minh (`src/convert.rs`)**:
  - Kéo nhiều PDF: Tự động đưa **Gộp PDF** lên đầu danh sách lựa chọn.
  - Kéo nhiều tệp ảnh, video, PDF hoặc hỗn hợp: Tự động xuất hiện tùy chọn **Nén** để nén hàng loạt song song tối đa 4 luồng.
- **Thùng rác & Hủy tác vụ từ khay (`src/ui.rs`)**:
  - Thêm tùy chọn **Đưa tệp gốc vào Thùng rác sau khi chuyển** (lưu trong `recycle.txt`; chỉ chạy khi mọi tệp đích đã lưu thành công và nguồn nằm trên ổ đĩa cục bộ `DRIVE_FIXED`, dùng `SHFileOperationW` + `FOF_ALLOWUNDO`).
  - Thêm **Hủy tác vụ đang chạy** để dừng ngay cây tiến trình con đang chuyển đổi qua `taskkill.exe /F /T`.

### Nghiệm thu v3 trên Windows x64
- `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo test --locked --offline` (**13 unit tests**), `cargo build --locked --offline --release`, và `scripts/conversion-smoke.ps1` (gồm `MD → HTML/DOCX/PDF`, `PDF → DOCX/TXT`, `Merge PDF`, `Split PDF`, `Compress PDF/Ảnh/Video`, kiểm tra fallback read-only ACL, `.ts` TypeScript và `misnamed.mp4`) đều đạt 100%.
