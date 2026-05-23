use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ResponseData<T: Serialize> {
    pub data: T,
    pub success: bool,
    #[serde(rename = "retCode")]
    pub ret_code: String,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    #[serde(rename = "showType", skip_serializing_if = "Option::is_none")]
    pub show_type: Option<u8>,
}

impl<T: Serialize> ResponseData<T> {
    pub fn success(data: T) -> Self {
        Self {
            data,
            success: true,
            ret_code: "0".to_string(),
            ret_msg: "ok".to_string(),
            show_type: None,
        }
    }

    pub fn error(data: T, code: &str, msg: &str) -> Self {
        Self {
            data,
            success: false,
            ret_code: code.to_string(),
            ret_msg: msg.to_string(),
            show_type: Some(2),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PaginationData<T: Serialize> {
    pub limit: i64,
    pub offset: i64,
    #[serde(rename = "pageNo")]
    pub page_no: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
    pub pages: i64,
    pub records: Vec<T>,
    #[serde(rename = "totalCount")]
    pub total_count: i64,
}

impl<T: Serialize> PaginationData<T> {
    pub fn new(records: Vec<T>, total_count: i64, page_no: i64, page_size: i64) -> Self {
        let pages = if total_count == 0 {
            0
        } else {
            (total_count + page_size - 1) / page_size
        };
        let offset = (page_no - 1) * page_size;
        Self {
            limit: page_size,
            offset,
            page_no,
            page_size,
            pages,
            records,
            total_count,
        }
    }
}
