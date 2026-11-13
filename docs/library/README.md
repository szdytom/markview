# Integrate Markview

| Task | Guide |
| --- | --- |
| Mount a Web reader with explicit fonts and image transport | [Component setup](mvaac.md) |
| Embed editing or synchronize a separate editor | [Editor integration](editor.md) |
| Navigate source positions and headings | [Source coordinates and TOC](mvaac-source-api.md) |
| Supply asynchronous images | [Resource protocol](mvaac-resources.md) |
| Export unsaved content or manage fonts through the CLI | [Native export protocol](native-export.md) |

Web editing belongs to `@markview/editor`; the native application stays read-only.
Exact TypeScript signatures come from the package declarations. These guides own
cross-API behavior: units, lifecycle, resource ownership and invalidation.
Use [MVSS](../users/stylesheets.md) for stylesheet syntax and
[Web development](../developers/web.md) to build this checkout.

[All documentation](../README.md)
