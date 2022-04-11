fluent_templates::static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
    };
}

lazy_static! {
    pub static ref SYSTEM_LOCALE: unic_langid::LanguageIdentifier = {
        let locale = sys_locale::get_locale()
            .unwrap_or_else(|| String::from(""))
            .parse()
            .unwrap_or_else(|_| unic_langid::langid!("en-US"));
        log::info!("the detected system locale is: {}", locale);
        locale
    };
}
