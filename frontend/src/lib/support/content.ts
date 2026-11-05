export interface KbCategory { id: string; slug: string; name: string; description: string; sort_order: number; article_count: number }
export interface KbArticle {
  id: string; category_id: string; slug: string; title: string; body: string; locale: string | null;
  is_published: boolean; views: number; helpful_yes: number; helpful_no: number; updated_at: string;
}
export interface KbListItem { id: string; category_id: string; slug: string; title: string; excerpt: string; views: number; updated_at: string }
export interface Announcement { id: string; title: string; body: string; locale: string | null; is_published: boolean; published_at: string }
