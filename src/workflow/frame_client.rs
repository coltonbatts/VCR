use std::path::Path;

use anyhow::Result;

use crate::workflow::types::FrameUploadResult;

pub async fn maybe_upload_render_to_frame_io(
    frame_project_id: Option<&str>,
    frame_token: Option<&str>,
    output_file: &Path,
) -> Result<FrameUploadResult> {
    let Some(project_id) = frame_project_id else {
        return Ok(FrameUploadResult {
            uploaded: false,
            link: None,
            note: "Frame.io upload skipped (no --frame-project supplied).".to_owned(),
        });
    };

    if frame_token.is_none() {
        return Ok(FrameUploadResult {
            uploaded: false,
            link: None,
            note: format!(
                "Frame.io upload skipped for project '{project_id}' because FRAME_IO_TOKEN is not set."
            ),
        });
    }

    Ok(FrameUploadResult {
        uploaded: false,
        link: None,
        note: format!(
            "Frame.io upload placeholder: project '{project_id}' requested, output '{}'. Upload API wiring is not implemented in MVP yet.",
            output_file.display()
        ),
    })
}
