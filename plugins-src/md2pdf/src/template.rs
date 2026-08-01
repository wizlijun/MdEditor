//! Wrap the host-supplied inline body HTML in a self-contained document the
//! offscreen WKWebView can load. The print stylesheet is embedded into the
//! binary at build time (include_str!) so the CLI has no runtime asset
//! dependency.

const PDF_CSS: &str = include_str!("../assets/pdf.css");

pub fn wrap_html(body: &str, title: &str) -> String {
    let title = html_escape(title);
    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <title>{title}</title>\n\
         <style>{PDF_CSS}</style>\n\
         </head>\n\
         <body data-pdf-title=\"{title}\">\n\
         {body}\n\
         </body>\n\
         </html>"
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
