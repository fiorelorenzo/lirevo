use crate::AppError;

#[tauri::command]
pub fn check_accessibility() -> Result<String, AppError> {
    Ok("not_determined".to_string()) // T12 fills with real impl
}

#[tauri::command]
pub fn prompt_accessibility() -> Result<String, AppError> {
    Ok("not_determined".to_string()) // T12 fills
}

#[tauri::command]
pub fn check_microphone() -> Result<String, AppError> {
    Ok("not_determined".to_string()) // T12 fills
}
