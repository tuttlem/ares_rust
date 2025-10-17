#[macro_export]
macro_rules! klog {
    ($($arg:tt)*) => {
        #[cfg(feature = "std")]
        {
            std::println!($($arg)*);
        }
    };
}
