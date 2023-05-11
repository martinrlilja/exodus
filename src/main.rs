use anyhow::{anyhow, Error, Result};
use fast_qr::{
    convert::{svg::SvgBuilder, Builder, Shape},
    qr::QRBuilder,
};
use gloo::file::{callbacks::FileReader, File};
use percent_encoding::utf8_percent_encode;
use prost::Message;
use wasm_bindgen_futures::JsFuture;
use web_sys::{DragEvent, Event, FileList, HtmlInputElement};
use yew::prelude::*;

use std::collections::HashMap;

use proto::{MigrationPayload, OtpAlgorithm, OtpDigitCount, OtpParameters, OtpType};

mod proto;

pub enum Msg {
    Files(Vec<File>),
    Loaded(String, Vec<u8>),
    ShowSvg(String),
    Copied(String, CopyState),
}

pub struct App {
    readers: HashMap<String, FileReader>,
    output: Vec<Output>,
    error: Option<String>,
}

pub struct Output {
    issuer: String,
    name: String,
    secret: String,
    kind: String,
    algorithm: Option<String>,
    digit_count: Option<String>,
    url: String,
    svg: String,
    show_svg: bool,
    copied: Option<CopyState>,
}

#[derive(Copy, Clone)]
pub enum CopyState {
    Copied,
    Failed,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            readers: HashMap::new(),
            output: Vec::new(),
            error: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Files(files) => {
                self.readers.clear();
                self.output.clear();

                for file in files.into_iter() {
                    let file_name = file.name();

                    let task = {
                        let link = ctx.link().clone();
                        let file_name = file_name.clone();

                        gloo::file::callbacks::read_as_bytes(&file, move |res| {
                            link.send_message(Msg::Loaded(
                                file_name,
                                res.expect("failed to read file"),
                            ))
                        })
                    };
                    self.readers.insert(file_name, task);
                }
                true
            }
            Msg::Loaded(file_name, buffer) => {
                if self.readers.remove(&file_name).is_none() {
                    false
                } else {
                    match Self::migration_from_file(buffer) {
                        Ok(Some(migration)) => {
                            let mut errors = vec![];

                            for params in migration.otp_parameters {
                                match Self::migration_to_output(params) {
                                    Ok(output) => self.output.push(output),
                                    Err(err) => errors.push(format!("{}", err)),
                                }
                            }

                            match &errors[..] {
                                &[] => self.error = None,
                                &[ref error] => {
                                    self.error =
                                        Some(format!("One account could not be read: {}", error))
                                }
                                errors => {
                                    self.error = Some(format!(
                                        "{} accounts could not be read: {}",
                                        errors.len(),
                                        errors.join(", ")
                                    ))
                                }
                            }
                        }
                        Ok(None) => {
                            self.output = vec![];
                            self.error = Some("No valid Google Authenticator Export QR code found in the uploaded image.".to_owned());
                        }
                        Err(err) => {
                            self.output = vec![];
                            self.error = Some(format!("Unknown error: {}", err));
                        }
                    }
                    true
                }
            }
            Msg::ShowSvg(url) => {
                for output in self.output.iter_mut() {
                    if output.url == url {
                        output.show_svg = !output.show_svg;
                    } else {
                        output.show_svg = false;
                    }
                }
                true
            }
            Msg::Copied(url, state) => {
                for output in self.output.iter_mut() {
                    if output.url == url {
                        output.copied = Some(state);
                    } else {
                        output.copied = None;
                    }
                }
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <label class="upload-wrapper" for="file-upload">
                    <div
                        class="upload"
                        ondrop={ctx.link().callback(|event: DragEvent| {
                            event.prevent_default();
                            let files = event.data_transfer().unwrap().files();
                            Self::collect_files(files)
                        })}
                        ondragover={Callback::from(|event: DragEvent| {
                            event.prevent_default();
                        })}
                        ondragenter={Callback::from(|event: DragEvent| {
                            event.prevent_default();
                        })}
                    >
                        <p>{"Drop your images here or click to select"}</p>
                    </div>
                </label>
                <input
                    id="file-upload"
                    type="file"
                    multiple=true
                    accept="image/jpeg,image/png"
                    onchange={ctx.link().callback(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        Self::collect_files(input.files())
                    })}
                />
                if let Some(ref error) = self.error {
                    <p>{error}</p>
                }
                if !self.output.is_empty() {
                    <div class="output">
                        { for self.output.iter().map(|o| Self::view_output(ctx, o)) }
                    </div>
                }
            </div>
        }
    }
}

impl App {
    fn collect_files(files: Option<FileList>) -> Msg {
        if let Some(files) = files {
            let files = js_sys::try_iter(&files)
                .unwrap()
                .unwrap()
                .map(|v| web_sys::File::from(v.unwrap()))
                .map(File::from)
                .collect();

            Msg::Files(files)
        } else {
            Msg::Files(vec![])
        }
    }

    fn view_output(ctx: &Context<Self>, output: &Output) -> Html {
        let url = output.url.clone();
        let show_qr_code = ctx.link().callback(move |_| Msg::ShowSvg(url.clone()));

        let url = output.url.clone();
        let copy_as_text = ctx.link().callback_future(move |_| {
            let url = url.clone();
            async move {
                let navigator = web_sys::window().unwrap().navigator();

                let Some(clipboard) = navigator.clipboard() else {
                    return Msg::Copied(url.clone(), CopyState::Failed);
                };

                let copy_result: JsFuture = clipboard.write_text(&url).into();

                let state = match copy_result.await {
                    Ok(_) => CopyState::Copied,
                    Err(_) => CopyState::Failed,
                };

                Msg::Copied(url.clone(), state)
            }
        });

        html! {
            <div class="otp">
                <h2 class="otp__name">{&output.issuer} {" "} {&output.name}</h2>
                <p>
                    <button class="otp__show-qr-code" onclick={show_qr_code.clone()}>
                        {"Show QR code"}
                    </button>
                    {" "}
                    <button class="otp__copy" onclick={copy_as_text}>
                        {"Copy as text"}
                    </button>
                    if let Some(CopyState::Copied) = output.copied {
                        {" "}
                        <span class="otp__copied">{"Copied!"}</span>
                    } else if let Some(CopyState::Failed) = output.copied {
                        {" "}
                        <span class="otp__copied">{"Could not copy, use the URL in details."}</span>
                    }
                </p>
                <details>
                    <summary>{"Details"}</summary>
                    <dl>
                        if !output.issuer.is_empty() {
                            <dt>{"Issuer"}</dt>
                            <dd>{&output.issuer}</dd>
                        }
                        <dt>{"Name"}</dt>
                        <dd>{&output.name}</dd>
                        <dt>{"Type"}</dt>
                        <dd>{&output.kind}</dd>
                        if let Some(ref algorithm) = output.algorithm {
                            <dt>{"Algorithm"}</dt>
                            <dd>{algorithm}</dd>
                        }
                        if let Some(ref digit_count) = output.digit_count {
                            <dt>{"Digits"}</dt>
                            <dd>{digit_count}</dd>
                        }
                        <dt>{"Secret"}</dt>
                        <dd>{&output.secret}</dd>
                        <dt>{"URL"}</dt>
                        <dd>{&output.url}</dd>
                    </dl>
                </details>
                <div class="otp__qr-code_wrapper">
                    <img
                        class={classes!(
                            "otp__qr-code",
                            if output.show_svg {
                                Some("otp__qr-code--show")
                            } else {
                                None
                            }
                        )}
                        onclick={show_qr_code}
                        src={output.svg.clone()}
                        alt="One time pad QR code" />
                </div>
            </div>
        }
    }

    fn migration_from_file(buffer: Vec<u8>) -> Result<Option<MigrationPayload>> {
        fn extract(buffer: &[u8], threshold: bool) -> Result<Option<MigrationPayload>> {
            let mut img = image::load_from_memory(&buffer)?.to_luma8();

            if threshold {
                for pixel in img.pixels_mut() {
                    if pixel[0] > 128 {
                        pixel[0] = 255;
                    } else {
                        pixel[0] = 0;
                    }
                }
            }

            let mut img = rqrr::PreparedImage::prepare(img);

            let grids = img.detect_grids();

            let migration = grids
                .into_iter()
                .flat_map(|grid| {
                    let (_meta, content) = grid.decode()?;

                    let url = url::Url::parse(&content)?;

                    Ok::<_, Error>(url)
                })
                .filter(|url| url.scheme() == "otpauth-migration")
                .flat_map(|url| {
                    let (_, data) = url
                        .query_pairs()
                        .find(|(key, _)| key == "data")
                        .ok_or_else(|| anyhow!("could not find data param in url"))?;

                    let message = base64::decode(data.as_ref())?;

                    let migration = MigrationPayload::decode(message.as_slice())?;

                    Ok::<_, Error>(migration)
                })
                .next();

            Ok(migration)
        }

        let migration = extract(&buffer, false)?;

        if migration.is_some() {
            Ok(migration)
        } else {
            // If we fail to parse the image, try to run a basic threshold filter
            // to counteract any JPEG compression artefacts.
            extract(&buffer, true)
        }
    }

    fn migration_to_output(params: OtpParameters) -> Result<Output> {
        let secret = base32::encode(base32::Alphabet::RFC4648 { padding: false }, &params.secret);

        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("secret", secret.as_str());

        let kind = match params.type_() {
            OtpType::Unspecified => return Err(anyhow!("unknown otp type")),
            OtpType::Hotp => {
                query.append_pair("counter", &params.counter.to_string());
                "hotp"
            }
            OtpType::Totp => "totp",
        };

        let name = if !params.issuer.is_empty() {
            query.append_pair("issuer", &params.issuer);

            utf8_percent_encode(
                &format!("{}:{}", params.issuer, params.name),
                percent_encoding::NON_ALPHANUMERIC,
            )
            .to_string()
        } else {
            utf8_percent_encode(&params.name, percent_encoding::NON_ALPHANUMERIC).to_string()
        };

        let algorithm = match params.algorithm() {
            OtpAlgorithm::Unspecified => None,
            OtpAlgorithm::Sha1 => Some("SHA1"),
            OtpAlgorithm::Sha256 => Some("SHA256"),
            OtpAlgorithm::Sha512 => Some("SHA512"),
            OtpAlgorithm::Md5 => Some("MD5"),
        };

        if let Some(algorithm) = algorithm {
            query.append_pair("algorithm", algorithm);
        }

        let digit_count = match params.digits() {
            OtpDigitCount::Unspecified => None,
            OtpDigitCount::Six => Some("6"),
            OtpDigitCount::Eight => Some("8"),
        };

        if let Some(digit_count) = digit_count {
            query.append_pair("digits", digit_count);
        }

        let querystring = query.finish();

        // https://github.com/google/google-authenticator/wiki/Key-Uri-Format
        let url = format!("otpauth://{kind}/{name}?{}", querystring);

        let qrcode = QRBuilder::new(url.clone())
            .ecl(fast_qr::ECL::L)
            .build()
            .unwrap();

        let svg = SvgBuilder::default().shape(Shape::Square).to_str(&qrcode);

        let svg = format!(
            "data:image/svg+xml,{}",
            percent_encoding::utf8_percent_encode(&svg, percent_encoding::NON_ALPHANUMERIC)
        )
        .to_string();

        Ok(Output {
            issuer: params.issuer,
            name: params.name,
            kind: kind.to_uppercase(),
            algorithm: algorithm.map(Into::into),
            digit_count: digit_count.map(Into::into),
            secret,
            url,
            svg,
            show_svg: false,
            copied: None,
        })
    }
}

fn main() {
    let document = gloo::utils::document();

    let app = document.get_element_by_id("app").unwrap();

    yew::Renderer::<App>::with_root(app).render();
}
