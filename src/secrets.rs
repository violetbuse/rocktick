pub struct Secret {
    id: String,
    master_key_id: i32,
    secret_version: i32,
    encrypted_dek: Vec<u8>,
    encrypted_data: Vec<u8>,
    algorithm: String,
}
