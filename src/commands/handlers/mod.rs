macro_rules! pub_use_handler_modules {
    ( $( $name:ident ),* ) => {
        $(
            mod $name;
            pub use self::$name::*;
        )*
    };
}

pub_use_handler_modules!(misc, identity, channels, userqueries);
