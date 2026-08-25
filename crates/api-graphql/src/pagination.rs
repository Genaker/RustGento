/// Result of paginating a known-length collection: the `[start, end)` slice
/// bounds to apply, and `total_pages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub start: usize,
    pub end: usize,
    pub total_pages: i32,
}

/// Computes 1-indexed page slicing over `total` items, matching Go's manual
/// `paginate()` helper exactly: `total_pages = ceil(total/page_size)`,
/// floored at 1 (an empty result set is still "page 1 of 1", not "of 0").
/// A `page_size` or `current_page` of zero or negative is clamped to 1,
/// mirroring how Go's resolvers treat non-positive GraphQL int arguments
/// (which the schema defaults to 20/1, but a client can still pass `0`).
pub fn paginate(total: usize, page_size: i32, current_page: i32) -> Page {
    let page_size = page_size.max(1) as usize;
    let current_page = current_page.max(1) as usize;

    let total_pages = ((total as f64) / (page_size as f64)).ceil() as i32;
    let total_pages = total_pages.max(1);

    let start = (current_page - 1) * page_size;
    let start = start.min(total);
    let end = (start + page_size).min(total);

    Page { start, end, total_pages }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page_of_a_full_page_size() {
        let p = paginate(45, 20, 1);
        assert_eq!(p.start, 0);
        assert_eq!(p.end, 20);
        assert_eq!(p.total_pages, 3);
    }

    #[test]
    fn last_partial_page() {
        let p = paginate(45, 20, 3);
        assert_eq!(p.start, 40);
        assert_eq!(p.end, 45);
        assert_eq!(p.total_pages, 3);
    }

    #[test]
    fn page_past_the_end_yields_an_empty_slice() {
        let p = paginate(45, 20, 10);
        assert_eq!(p.start, 45);
        assert_eq!(p.end, 45);
        assert_eq!(p.total_pages, 3);
    }

    #[test]
    fn empty_total_is_page_one_of_one_not_zero() {
        let p = paginate(0, 20, 1);
        assert_eq!(p.start, 0);
        assert_eq!(p.end, 0);
        assert_eq!(p.total_pages, 1);
    }

    #[test]
    fn exact_multiple_does_not_add_a_trailing_empty_page() {
        let p = paginate(40, 20, 2);
        assert_eq!(p.total_pages, 2);
        assert_eq!(p.start, 20);
        assert_eq!(p.end, 40);
    }

    #[test]
    fn zero_or_negative_page_size_is_clamped_to_one() {
        let p = paginate(5, 0, 1);
        assert_eq!(p.total_pages, 5);
        assert_eq!(p.end - p.start, 1);

        let p = paginate(5, -3, 1);
        assert_eq!(p.total_pages, 5);
    }

    #[test]
    fn zero_or_negative_current_page_is_clamped_to_one() {
        let p = paginate(45, 20, 0);
        assert_eq!(p.start, 0);
        assert_eq!(p.end, 20);

        let p = paginate(45, 20, -5);
        assert_eq!(p.start, 0);
        assert_eq!(p.end, 20);
    }

    #[test]
    fn single_item_single_page() {
        let p = paginate(1, 20, 1);
        assert_eq!(p.start, 0);
        assert_eq!(p.end, 1);
        assert_eq!(p.total_pages, 1);
    }
}
