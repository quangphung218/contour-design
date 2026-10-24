# Hướng dẫn Contour

Contour biến một mô tả thành thiết kế: trang web, prototype, slide deck, tài liệu, ảnh và video. Mọi thứ chạy trên máy này. Agent lập trình đang dùng (Claude Code, Codex, Gemini CLI…) hoặc API key riêng làm việc; không có gì được tải lên trừ khi cấu hình provider.

## Bắt đầu

1. **Chọn agent.** Lần đầu mở, chọn một agent đã cài để Contour thử kết nối, hoặc dán API key.
2. **Thương hiệu (tuỳ chọn).** Nhập website, Contour trích design system (bảng màu, kiểu chữ, component). Project mới dùng nó từ đầu.
3. **Mô tả thứ cần làm** ở Home rồi bấm Tạo. Studio mở ra và agent bắt đầu.

## Home

- Các chip đầu ra (Prototype, Slide deck, Tài liệu, Ảnh, Video…) quyết định agent làm gì. Chip media có thêm tỉ lệ, thời lượng, độ phân giải.
- **+** đính file, plugin, skill, MCP server hoặc tài nguyên trong thư viện media. Gõ **@** để nhắc tới các thứ đó ngay trong câu, **/** để gọi lệnh.
- **Gợi ý** đề xuất một điểm bắt đầu theo những gì đã làm. Các tab bên dưới mở thiết kế, template, design system và template prompt ảnh hoặc video.

## Studio

- Chat bên trái, preview ở giữa, file bên phải.
- Thanh preview có Bình luận, Soi phần tử, Sửa chữ, Vẽ, Tweaks, Bảng màu và Lint. Bình luận được đính vào tin nhắn kế tiếp.
- **Export** lưu HTML, ZIP, PDF, ảnh, PowerPoint, MP4, Markdown, hoặc mã nguồn component React (`.jsx`/`.tsx`).
- **Design Jury** chấm thiết kế qua nhiều vòng trước khi giao. Bấm **Ngắt** (hoặc Esc) để dừng; vòng tốt nhất đến lúc đó được giao. **Xem lại** phát lại một lần chấm đã xong.
- Khi agent tạo ảnh, âm thanh hay video, một dòng trong chat hiện từng mục khi nó được tạo.

## Design system

Tạo từ website, repo GitHub, file, DESIGN.md hay biến CSS dán vào, hoặc chép một bộ có sẵn. Tải lên file font và logo; khi chưa có font thương hiệu, preview dùng font thay thế.

## Bộ nhớ

Settings ▸ Bộ nhớ giữ những điều mọi project cần tuân theo. Tắt bộ nhớ bằng công tắc chính. Memory hooks chọn phần nào tự chạy: hồ sơ, viết lại yêu cầu ngắn, tự kiểm theo quy tắc và học từ chat. **Nhập** đọc các mục từ file Markdown; **Sửa MEMORY.md** chọn mục nào vào prompt. Trong chat, mở đầu tin nhắn bằng "Ghi nhớ rằng…" để lưu một điều cho project đó.

## DeepSeek Harness {#deepseek-harness}

Contour chạy `dsh` CLI của người dùng qua profile `open-design`. Contour không tự cài cái nào.

1. Cài `dsh`. Bộ cài khởi tạo cài luôn Node.js và pnpm bản tương thích, rồi mở web UI của Harness để lưu DeepSeek API key:
   - macOS / Linux: `curl -fsSL 'https://open-design.ai/install-dsh.sh?version=1' | sh`
   - Windows PowerShell: `& ([scriptblock]::Create((irm 'https://open-design.ai/install-dsh.ps1?version=1')))`
2. Thêm profile `open-design`. Build gói của nó từ mã nguồn upstream (`nexu-io/open-design`, `packages/dsh-runtime`) rồi thêm bằng `dsh`:
   - `pnpm --filter @open-design/dsh-runtime build`
   - `pnpm -C packages/dsh-runtime pack --pack-destination <thư-mục>`
   - `dsh plugin --profile open-design add <thư-mục>/open-design-dsh-runtime-0.1.0.tgz`
   - `dsh --profile open-design --probe` in ra một dòng JSON khi profile chạy được.
3. Quay lại Contour, mở thẻ DeepSeek Harness và bấm **Phát hiện lại**. Agent được chọn ngay khi qua bước kiểm tra.

Profile nằm ở `~/.dsh/profiles/open-design`, hoặc trong `$DSH_HOME` nếu biến này được đặt.

## Bàn phím

- `Cmd/Ctrl+K` bảng lệnh, `?` mọi phím tắt, `Cmd/Ctrl+,` Settings.
- Trong Studio: `Cmd/Ctrl+B` cột file, `Cmd/Ctrl+T` tab mới, `Cmd/Ctrl+P` tìm file.

## Khi có lỗi

- **Agent không trả lời:** Settings ▸ Model ▸ Thử, hoặc Làm mới ở màn onboarding.
- **Export PDF hay ảnh lỗi:** cài Chrome, Edge hoặc Chromium, hoặc đặt `OD_CHROME_PATH`.
- **Báo lỗi:** Help ▸ Report Issue chép bản tóm tắt chẩn đoán (phiên bản, nền tảng, agent, các run gần đây, không có gì bí mật) vào clipboard và mở thư mục log của run. Dán nó vào nơi báo lỗi. Settings ▸ About ▸ Export diagnostics lưu bản lưu trữ đầy đủ.
