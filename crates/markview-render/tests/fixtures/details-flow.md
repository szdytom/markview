# Container flow

> Before the disclosure.
>
> <details open>
> <summary>Outer <b>summary</b> with a <a href="https://example.invalid">link</a></summary>
>
> First body paragraph with **strong**, `code`, and $x^2$.
>
> <details>
> <summary>Inner summary</summary>
>
> ![Tiles](tiles.png "Nested image")
>
> | Key | Value |
> | --- | --- |
> | Nested | table |
>
> </details>
>
> After the inner disclosure.
>
> </details>
>
> After the outer disclosure.

1. Before the list disclosure.

   <details open>
   <summary>List summary</summary>

   ```rust
   let message = "A deliberately long code line that keeps its horizontal scroll position across disclosure toggles.";
   ```

   - [x] A nested task
   - Another item

   </details>

   After the list disclosure.

2. Following list item.

<details>
<summary>Closed sibling</summary>

## Hidden heading

Hidden body with a [link](https://example.invalid).

</details>

Text after all containers.
