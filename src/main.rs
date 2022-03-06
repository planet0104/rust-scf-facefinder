use anyhow::Result;
use facefinder::{Opt, PostData};
use log::{error, info};

mod facefinder;
mod config;
use config::CONFIG;
use reqwest::Response;
use serde_json::{json, Value};
use tokio::{time::{ sleep, Duration }};

#[tokio::main]
async fn main(){
    env_logger::init();

    //通知初始化成功
    let _ = post_data(&CONFIG.ready_url, &json!({ "msg": "facefinder ready"})).await;

    loop{
        //循环读取事件
        match reqwest::get(&CONFIG.event_url).await{
            Ok(response) => {
                if let Err(err) = process_event(response).await{
                    let err_res = post_data(&CONFIG.error_url, &json!({"msg": format!("{:?}", err)})).await;
                    error!("event 处理失败 {:?} res={:?}", err, err_res);
                }
            }
            Err(err) => {
                error!("event 读取失败 {:?}", err);
                sleep(Duration::from_millis(1000)).await;
            }
        }
    }
}

async fn process_event(response: Response) -> Result<()> {
    let mut event: Value = response.json().await?;

    info!("获取到event:{event}");

    // 网关api数据存储在body字段中
    let body_str = event["body"].as_str().unwrap_or("");
    if body_str.len() > 0{
        if let Ok(e) = serde_json::from_str(body_str){
            event = e;
        }
    }

    /*
    提交数据:PostData
     */
    let resp = match serde_json::from_value::<PostData>(event){
        Ok(data) => {
            let mut opt = Opt::default();

            if let Some(min_size) = data.min_size{
                opt.min_size = min_size;
            }
            if let Some(scale_factor) = data.scale_factor{
                opt.scale_factor = scale_factor;
            }
            if let Some(shift_factor) = data.shift_factor{
                opt.shift_factor = shift_factor;
            }
            if let Some(threshold) = data.threshold{
                opt.threshold = threshold;
            }

            match facefinder::detect_faces(&opt, &data.img){
                Ok(faces) => serde_json::to_value(faces).unwrap_or(Value::Array(vec![])),
                Err(err) => json!({
                    "error": format!("{:?}", err)
                }),
            }
        }
        Err(err) => {
            json!({
                "error": format!("{:?}", err)
            })
        }
    };
    
    let data = post_data(&CONFIG.response_url, &json!({
        "isBase64Encoded": false,
        "statusCode": 200,
        "headers": {"Content-Type":"application/json"},
        "body": resp.to_string()
    })).await?;
    info!("invoke response: {data}");
    Ok(())
}

async fn post_data(url: &str, data: &Value) -> Result<String> {
    let client = reqwest::Client::new();
    info!("返回数据:{:?}", data);
    let res = client.post(url).json(data).send().await?;
    Ok(res.text().await?)
}
