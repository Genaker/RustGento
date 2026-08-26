use std::cmp::{max, min};

/// Computed pagination window over an item list -- mirrors Go's
/// `calculatePagination`: clamps the requested page into range and shows
/// at most 5 page-number links, centered on the current page where
/// possible. Shared by the category and search pages.
pub struct Pagination {
    pub page: usize,
    pub limit: usize,
    pub total_pages: usize,
    pub page_numbers: Vec<usize>,
    pub prev_page: usize,
    pub next_page: usize,
}

pub fn paginate(total_items: usize, requested_page: usize, limit: usize) -> Pagination {
    let limit = limit.max(1);
    let total_pages = total_items.div_ceil(limit).max(1);
    let page = requested_page.clamp(1, total_pages);

    const MAX_PAGES_SHOWN: usize = 5;
    let start_page = max(1, page as isize - MAX_PAGES_SHOWN as isize / 2) as usize;
    let mut end_page = min(total_pages, start_page + MAX_PAGES_SHOWN - 1);
    let start_page = if end_page - start_page + 1 < MAX_PAGES_SHOWN { max(1, end_page as isize - MAX_PAGES_SHOWN as isize + 1) as usize } else { start_page };
    if end_page < start_page {
        end_page = start_page;
    }

    Pagination {
        page,
        limit,
        total_pages,
        page_numbers: (start_page..=end_page).collect(),
        prev_page: page.saturating_sub(1).max(1),
        next_page: (page + 1).min(total_pages),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_defaults_to_page_one() {
        let p = paginate(45, 1, 20);
        assert_eq!(p.page, 1);
        assert_eq!(p.total_pages, 3);
        assert_eq!(p.page_numbers, vec![1, 2, 3]);
        assert_eq!(p.prev_page, 1);
        assert_eq!(p.next_page, 2);
    }

    #[test]
    fn paginate_clamps_page_beyond_total() {
        let p = paginate(10, 99, 20);
        assert_eq!(p.page, 1);
        assert_eq!(p.total_pages, 1);
    }

    #[test]
    fn paginate_clamps_page_below_one() {
        let p = paginate(45, 0, 20);
        assert_eq!(p.page, 1);
    }

    #[test]
    fn paginate_shows_at_most_five_page_numbers_centered_on_current() {
        let p = paginate(400, 10, 20); // 20 total pages, on page 10
        assert_eq!(p.page_numbers.len(), 5);
        assert!(p.page_numbers.contains(&10));
        assert_eq!(p.prev_page, 9);
        assert_eq!(p.next_page, 11);
    }

    #[test]
    fn paginate_page_numbers_near_the_end_dont_run_past_total() {
        let p = paginate(400, 20, 20); // last page
        assert_eq!(*p.page_numbers.last().unwrap(), 20);
        assert_eq!(p.page_numbers.len(), 5);
        assert_eq!(p.next_page, 20);
    }

    #[test]
    fn paginate_single_page_has_no_prev_or_next() {
        let p = paginate(5, 1, 20);
        assert_eq!(p.total_pages, 1);
        assert_eq!(p.prev_page, 1);
        assert_eq!(p.next_page, 1);
    }
}
