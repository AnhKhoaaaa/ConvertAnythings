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
