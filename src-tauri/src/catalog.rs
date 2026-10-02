use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use storelib_rs::{
    DCatEndpoint, DCatSearch, DisplayCatalogHandler, DisplayCatalogModel, IdentifierType, Lang,
    Locale, Market,
};

pub type CatalogFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The device families supported by the project boundary. The adapter maps
/// these values to storelib_rs only at the protocol edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceFamily {
    Desktop,
    Mobile,
    Xbox,
    ServerCore,
    IotCore,
    HoloLens,
    Andromeda,
    Universal,
    Wcos,
}

impl DeviceFamily {
    pub const fn as_platform_dependency_name(self) -> &'static str {
        match self {
            Self::Desktop => "Windows.Desktop",
            Self::Mobile => "Windows.Mobile",
            Self::Xbox => "Windows.Xbox",
            Self::ServerCore => "Windows.Server",
            Self::IotCore => "Windows.Iot",
            Self::HoloLens => "Windows.Holographic",
            Self::Andromeda => "Windows.8828080",
            Self::Universal => "Windows.Universal",
            Self::Wcos => "Windows.Core",
        }
    }
}

impl From<DeviceFamily> for storelib_rs::DeviceFamily {
    fn from(value: DeviceFamily) -> Self {
        match value {
            DeviceFamily::Desktop => Self::Desktop,
            DeviceFamily::Mobile => Self::Mobile,
            DeviceFamily::Xbox => Self::Xbox,
            DeviceFamily::ServerCore => Self::ServerCore,
            DeviceFamily::IotCore => Self::IotCore,
            DeviceFamily::HoloLens => Self::HoloLens,
            DeviceFamily::Andromeda => Self::Andromeda,
            DeviceFamily::Universal => Self::Universal,
            DeviceFamily::Wcos => Self::Wcos,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogProduct {
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub package_formats: Vec<String>,
    pub framework_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogIdentifier {
    ProductId,
    PackageFamilyName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    MalformedFixture(String),
    MissingField(&'static str),
    InvalidUrl { field: &'static str },
    UnsupportedLocale,
    StoreLib,
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedFixture(message) => {
                write!(formatter, "malformed catalog fixture: {message}")
            }
            Self::MissingField(field) => write!(formatter, "catalog field is missing: {field}"),
            Self::InvalidUrl { field, .. } => write!(formatter, "catalog URL is invalid: {field}"),
            Self::UnsupportedLocale => formatter.write_str("Store locale is unsupported"),
            Self::StoreLib => formatter.write_str("store protocol request failed"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Stable protocol boundary consumed by later domain and Tauri layers.
pub trait CatalogProvider {
    fn search<'a>(
        &'a mut self,
        query: &'a str,
        device_family: DeviceFamily,
    ) -> CatalogFuture<'a, Result<Vec<CatalogProduct>, CatalogError>>;

    fn lookup<'a>(
        &'a mut self,
        identifier: CatalogIdentifier,
        product_id: &'a str,
    ) -> CatalogFuture<'a, Result<CatalogProduct, CatalogError>>;

    fn product<'a>(
        &'a mut self,
        product_id: &'a str,
    ) -> CatalogFuture<'a, Result<CatalogProduct, CatalogError>> {
        self.lookup(CatalogIdentifier::ProductId, product_id)
    }
}

/// Adapter that is the only catalog module allowed to depend on storelib_rs.
pub struct StoreLibCatalogAdapter {
    handler: DisplayCatalogHandler,
}

impl StoreLibCatalogAdapter {
    pub fn production() -> Self {
        Self {
            handler: DisplayCatalogHandler::production(),
        }
    }

    pub fn production_for_locale(market: &str, language: &str) -> Result<Self, CatalogError> {
        let market = Market::from_str(&market.trim().to_ascii_uppercase())
            .map_err(|_| CatalogError::UnsupportedLocale)?;
        let language = language
            .split('-')
            .next()
            .ok_or(CatalogError::UnsupportedLocale)?
            .trim()
            .to_ascii_lowercase();
        let language = Lang::from_str(&language).map_err(|_| CatalogError::UnsupportedLocale)?;
        let locale = Locale::new(market, language, true).with_full_tag(true);
        Ok(Self {
            handler: DisplayCatalogHandler::new(DCatEndpoint::Production, locale),
        })
    }

    /// Parse a captured DisplayCatalog autosuggest response without network IO.
    pub fn parse_search_fixture(json: &str) -> Result<Vec<CatalogProduct>, CatalogError> {
        let response: DCatSearch = serde_json::from_str(json)
            .map_err(|error| CatalogError::MalformedFixture(error.to_string()))?;
        response
            .results
            .unwrap_or_default()
            .into_iter()
            .flat_map(|group| group.products.unwrap_or_default())
            .map(|product| normalize_product(&product))
            .collect()
    }

    /// Parse a captured DisplayCatalog product response without network IO.
    pub fn parse_product_fixture(json: &str) -> Result<CatalogProduct, CatalogError> {
        let response: DisplayCatalogModel = serde_json::from_str(json)
            .map_err(|error| CatalogError::MalformedFixture(error.to_string()))?;
        let product = response
            .products
            .as_deref()
            .and_then(|products| products.first())
            .or(response.product.as_ref())
            .ok_or(CatalogError::MissingField("product"))?;
        normalize_product(product)
    }
}

impl Default for StoreLibCatalogAdapter {
    fn default() -> Self {
        Self::production()
    }
}

impl CatalogProvider for StoreLibCatalogAdapter {
    fn search<'a>(
        &'a mut self,
        query: &'a str,
        device_family: DeviceFamily,
    ) -> CatalogFuture<'a, Result<Vec<CatalogProduct>, CatalogError>> {
        Box::pin(async move {
            let response = self
                .handler
                .search_dcat(query, device_family.into())
                .await
                .map_err(|_| CatalogError::StoreLib)?;

            response
                .results
                .unwrap_or_default()
                .into_iter()
                .flat_map(|group| group.products.unwrap_or_default())
                .map(|product| normalize_product(&product))
                .collect()
        })
    }

    fn lookup<'a>(
        &'a mut self,
        identifier: CatalogIdentifier,
        product_id: &'a str,
    ) -> CatalogFuture<'a, Result<CatalogProduct, CatalogError>> {
        Box::pin(async move {
            if product_id.trim().is_empty() {
                return Err(CatalogError::MissingField("productId"));
            }

            let identifier_type = match identifier {
                CatalogIdentifier::ProductId => IdentifierType::ProductId,
                CatalogIdentifier::PackageFamilyName => IdentifierType::PackageFamilyName,
            };
            self.handler
                .query_dcat(product_id, identifier_type, None)
                .await
                .map_err(|_| CatalogError::StoreLib)?;
            let product = self
                .handler
                .product()
                .ok_or(CatalogError::MissingField("product"))?;
            normalize_product(product)
        })
    }
}

fn normalize_product(product: &storelib_rs::Product) -> Result<CatalogProduct, CatalogError> {
    let product_id = product
        .product_id
        .clone()
        .or_else(|| {
            product.alternate_ids.as_deref()?.iter().find_map(|id| {
                let id_type = id.id_type.as_deref()?.to_ascii_lowercase();
                (id_type == "productid").then(|| id.value.clone()).flatten()
            })
        })
        .filter(|id| !id.trim().is_empty())
        .ok_or(CatalogError::MissingField("productId"))?;

    let sku_properties = product
        .display_sku_availabilities
        .as_deref()
        .and_then(|availabilities| availabilities.first())
        .and_then(|availability| availability.sku.as_ref())
        .and_then(|sku| sku.properties.as_ref());
    let packages = sku_properties
        .and_then(|properties| properties.packages.as_deref())
        .unwrap_or_default();

    let mut package_formats = Vec::new();
    let mut framework_dependencies = Vec::new();
    for package in packages {
        if let Some(url) = package.package_uri.as_deref() {
            validate_download_url(url, "packageUri")?;
        }
        if let Some(format) = package.package_format.as_deref() {
            let format = format.to_ascii_lowercase();
            if !package_formats.contains(&format) {
                package_formats.push(format);
            }
        }
        for dependency in package
            .framework_dependencies
            .as_deref()
            .unwrap_or_default()
        {
            if let Some(identity) = dependency
                .package_identity
                .as_deref()
                .filter(|identity| !identity.trim().is_empty())
            {
                if !framework_dependencies.iter().any(|item| item == identity) {
                    framework_dependencies.push(identity.to_owned());
                }
            }
        }
    }

    Ok(CatalogProduct {
        product_id,
        package_family_name: product
            .properties
            .as_ref()
            .and_then(|properties| properties.package_family_name.clone())
            .or_else(|| {
                packages
                    .iter()
                    .find_map(|package| package.package_family_name.clone())
            }),
        title: product
            .localized_properties
            .as_deref()
            .and_then(|properties| properties.first())
            .and_then(|property| property.product_title.clone())
            .or_else(|| product.title.clone()),
        publisher: product
            .localized_properties
            .as_deref()
            .and_then(|properties| properties.first())
            .and_then(|property| property.publisher_name.clone()),
        package_formats,
        framework_dependencies,
    })
}

fn validate_download_url(value: &str, field: &'static str) -> Result<(), CatalogError> {
    let valid = value
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace));
    if valid {
        Ok(())
    } else {
        Err(CatalogError::InvalidUrl { field })
    }
}
