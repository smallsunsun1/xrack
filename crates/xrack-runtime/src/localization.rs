use crate::{Property, localized_number::LocalizedNumber};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use std::{
    borrow::Cow,
    cell::{OnceCell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

pub use fluent_bundle::FluentValue as TranslationValue;
pub use unic_langid::LanguageIdentifier as LocaleId;

/// The requested UI language shared by components on the UI thread.
/// Until a language is selected, each catalog uses its declared default.
#[derive(Clone, Default)]
pub struct LocaleContext(Rc<Property<Option<LocaleId>>>);

impl LocaleContext {
    /// Select a language for standalone components, such as offscreen renders.
    /// Desktop applications can use `Desktop::set_locale` to also wake the event loop.
    pub fn set_locale(&self, locale: &str) -> Result<(), String> {
        let locale = locale
            .parse::<LocaleId>()
            .map_err(|error| format!("invalid UI locale `{locale}`: {error}"))?;
        self.set_language(locale);
        Ok(())
    }

    #[doc(hidden)]
    pub fn set_language(&self, locale: LocaleId) {
        self.0.set_if_changed(Some(locale));
    }
}

/// One reactive connection shared by a generated component and its descendants.
#[derive(Clone, Default)]
pub struct LocaleBinding {
    context: Rc<Property<LocaleContext>>,
    number: Rc<RefCell<Option<(LocaleId, LocalizedNumber)>>>,
}

impl LocaleBinding {
    pub fn set_context(&self, context: LocaleContext) {
        self.context.set(context);
    }

    fn language(&self) -> Option<LocaleId> {
        self.context.get(LocaleContext::default).0.get(|| None)
    }

    #[doc(hidden)]
    pub fn locale(&self, catalog: Option<&TranslationCatalog>) -> String {
        let requested = self.language();
        match catalog {
            Some(catalog) => catalog.languages(requested).first().unwrap().to_string(),
            None => requested.map_or_else(|| "und".to_owned(), |locale| locale.to_string()),
        }
    }

    fn with_number<T>(
        &self,
        catalog: Option<&TranslationCatalog>,
        use_number: impl FnOnce(&LocalizedNumber) -> T,
    ) -> T {
        // Keep regional number conventions even when the translation catalog
        // resolves a requested language to a less specific tag.
        let language = self.language().unwrap_or_else(|| {
            catalog.map_or_else(LocaleId::default, |catalog| {
                catalog.locales[catalog.default].clone()
            })
        });
        let mut cache = self.number.borrow_mut();
        if cache.as_ref().is_none_or(|(cached, _)| *cached != language) {
            let formatter = LocalizedNumber::new(&language.to_string());
            *cache = Some((language, formatter));
        }
        use_number(&cache.as_ref().unwrap().1)
    }

    #[doc(hidden)]
    pub fn format_number_int(
        &self,
        catalog: Option<&TranslationCatalog>,
        value: i64,
        decimals: i64,
    ) -> String {
        let ascii = crate::format_fixed_int(value, decimals.clamp(0, 20) as usize);
        self.with_number(catalog, |format| format.format(ascii))
    }

    #[doc(hidden)]
    pub fn format_number_float(
        &self,
        catalog: Option<&TranslationCatalog>,
        value: f64,
        decimals: i64,
    ) -> String {
        let ascii = crate::format_fixed_float(value, decimals.clamp(0, 20) as usize);
        self.with_number(catalog, |format| format.format(ascii))
    }

    #[doc(hidden)]
    pub fn parse_number(
        &self,
        catalog: Option<&TranslationCatalog>,
        text: &str,
        fallback: f64,
    ) -> f64 {
        self.with_number(catalog, |format| format.parse(text))
            .unwrap_or(fallback)
    }
}

/// Parsed catalogs shared by all instances of a generated UI on one thread.
pub struct TranslationCatalog {
    locales: Vec<LocaleId>,
    sources: Vec<Cow<'static, str>>,
    bundles: Vec<OnceCell<FluentBundle<FluentResource>>>,
    default: usize,
}

impl TranslationCatalog {
    pub fn new(default: &str, sources: &[(&str, &'static str)]) -> Self {
        Self::from_sources(
            default,
            sources
                .iter()
                .map(|&(locale, text)| (locale, Cow::Borrowed(text))),
        )
        .expect("validated generated translation catalog")
    }

    pub fn from_owned(default: &str, sources: BTreeMap<String, String>) -> Result<Self, String> {
        let catalog = Self::from_sources(
            default,
            sources
                .into_iter()
                .map(|(locale, text)| (locale, Cow::Owned(text))),
        )?;
        for index in 0..catalog.locales.len() {
            let bundle = catalog.build_bundle(index)?;
            catalog.bundles[index]
                .set(bundle)
                .map_err(|_| "translation bundle was already initialized")?;
        }
        Ok(catalog)
    }

    /// Expose the catalog text to the XR resource validator.
    #[doc(hidden)]
    pub fn sources(&self) -> impl Iterator<Item = (&LocaleId, &str, bool)> {
        self.locales
            .iter()
            .zip(&self.sources)
            .enumerate()
            .map(|(index, (locale, source))| (locale, source.as_ref(), index == self.default))
    }

    fn from_sources(
        default: &str,
        sources: impl IntoIterator<Item = (impl AsRef<str>, Cow<'static, str>)>,
    ) -> Result<Self, String> {
        let mut default_index = None;
        let mut locales = Vec::new();
        let mut text = Vec::new();
        for (locale, source) in sources {
            let locale = locale.as_ref();
            if locale == default {
                default_index = Some(locales.len());
            }
            locales.push(
                locale
                    .parse::<LocaleId>()
                    .map_err(|error| error.to_string())?,
            );
            text.push(source);
        }
        let bundles = locales.iter().map(|_| OnceCell::new()).collect();
        Ok(Self {
            locales,
            sources: text,
            bundles,
            default: default_index.ok_or("translation catalog missing default language")?,
        })
    }

    fn build_bundle(&self, index: usize) -> Result<FluentBundle<FluentResource>, String> {
        let resource = FluentResource::try_new(self.sources[index].to_string())
            .map_err(|(_, errors)| format!("invalid Fluent resource: {errors:?}"))?;
        let mut bundle = FluentBundle::new(vec![self.locales[index].clone()]);
        bundle
            .add_resource(resource)
            .map_err(|errors| format!("invalid Fluent messages: {errors:?}"))?;
        bundle
            .add_builtins()
            .map_err(|errors| format!("invalid Fluent builtins: {errors:?}"))?;
        Ok(bundle)
    }

    fn languages(&self, requested: Option<LocaleId>) -> Vec<&LocaleId> {
        let fallback = &self.locales[self.default];
        let mut locales = match requested {
            Some(language) => negotiate_languages(
                &[language],
                &self.locales,
                None,
                NegotiationStrategy::Lookup,
            ),
            None => Vec::new(),
        };
        if !locales.contains(&fallback) {
            locales.push(fallback);
        }
        locales
    }

    pub fn translate(
        &self,
        locale: &LocaleBinding,
        id: &str,
        args: &[(&str, TranslationValue<'_>)],
    ) -> String {
        let args: FluentArgs<'_> = args.iter().cloned().collect();
        for language in self.languages(locale.language()) {
            let index = self
                .locales
                .iter()
                .position(|candidate| candidate == language)
                .expect("negotiated a catalog language");
            let bundle = self.bundles[index]
                .get_or_init(|| self.build_bundle(index).expect("validated Fluent resource"));
            if let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) {
                let mut errors = Vec::new();
                let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
                if errors.is_empty() {
                    return text.into_owned();
                }
            }
        }
        id.to_owned()
    }
}
