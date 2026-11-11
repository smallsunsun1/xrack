use crate::Property;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use std::{cell::OnceCell, rc::Rc};

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
pub struct LocaleBinding(Rc<Property<LocaleContext>>);

impl LocaleBinding {
    pub fn set_context(&self, context: LocaleContext) {
        self.0.set(context);
    }

    fn language(&self) -> Option<LocaleId> {
        self.0.get(LocaleContext::default).0.get(|| None)
    }
}

/// Parsed catalogs shared by all instances of a generated UI on one thread.
pub struct TranslationCatalog {
    locales: Vec<LocaleId>,
    sources: Vec<&'static str>,
    bundles: Vec<OnceCell<FluentBundle<FluentResource>>>,
    default: usize,
}

impl TranslationCatalog {
    pub fn new(default: &str, sources: &[(&str, &'static str)]) -> Self {
        let default = sources
            .iter()
            .position(|(locale, _)| *locale == default)
            .expect("generated catalog has a default language");
        let (locales, text) = sources
            .iter()
            .map(|(locale, source)| (locale.parse().expect("validated locale"), *source))
            .unzip();
        let bundles = sources.iter().map(|_| OnceCell::new()).collect();
        Self {
            locales,
            sources: text,
            bundles,
            default,
        }
    }

    pub fn translate(
        &self,
        locale: &LocaleBinding,
        id: &str,
        args: &[(&str, TranslationValue<'_>)],
    ) -> String {
        let fallback = &self.locales[self.default];
        let mut locales = match locale.language() {
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
        let args: FluentArgs<'_> = args.iter().cloned().collect();
        for language in locales {
            let index = self
                .locales
                .iter()
                .position(|candidate| candidate == language)
                .expect("negotiated a catalog language");
            let bundle = self.bundles[index].get_or_init(|| {
                let resource = FluentResource::try_new(self.sources[index].to_owned())
                    .expect("validated Fluent resource");
                let mut bundle = FluentBundle::new(vec![language.clone()]);
                bundle
                    .add_resource(resource)
                    .expect("validated Fluent message IDs");
                bundle.add_builtins().expect("validated Fluent builtins");
                bundle
            });
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
