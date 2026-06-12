/// Task 4.3 — LMS Chapter Progress Pathways
///
/// Full Learning Management System engine with courses, chapters, lessons,
/// quiz attempts, progress tracking, and certification eligibility.
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use thiserror::Error;

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Course {
    pub id: String,
    pub title: String,
    pub description: String,
    pub instructor: String,
    pub is_published: bool,
    pub passing_score: Decimal,
    pub certificate_template: Option<String>,
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub course_id: String,
    pub title: String,
    pub order_index: u32,
    pub lessons: Vec<Lesson>,
    pub quiz: Option<Quiz>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub chapter_id: String,
    pub title: String,
    pub content_type: ContentType,
    pub content_url: Option<String>,
    pub content_body: Option<String>,
    pub order_index: u32,
    pub duration_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Video,
    Article,
    Quiz,
    Assignment,
    LiveClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quiz {
    pub id: String,
    pub chapter_id: String,
    pub title: String,
    pub questions: Vec<QuizQuestion>,
    /// 0–100
    pub passing_score: Decimal,
    pub max_attempts: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub id: String,
    pub question_text: String,
    pub options: Vec<String>,
    /// Index of the correct option (zero-based)
    pub correct_option: usize,
    pub explanation: Option<String>,
}

// ── Progress tracking ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonCompletion {
    pub student: String,
    pub lesson_id: String,
    pub completed_at: DateTime<Utc>,
    pub time_spent_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizAttempt {
    pub student: String,
    pub quiz_id: String,
    pub answers: Vec<usize>,
    pub score: Decimal,
    pub passed: bool,
    pub attempted_at: DateTime<Utc>,
    pub attempt_number: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseProgress {
    pub student: String,
    pub course_id: String,
    /// 0.0 – 100.0
    pub completion_percentage: Decimal,
    pub completed_lessons: u32,
    pub total_lessons: u32,
    pub chapter_statuses: Vec<ChapterStatus>,
    pub is_certified: bool,
    pub certificate_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterStatus {
    pub chapter_id: String,
    pub chapter_title: String,
    pub lessons_done: u32,
    pub total_lessons: u32,
    pub quiz_passed: bool,
    pub is_unlocked: bool,
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum LmsError {
    #[error("Chapter {0} is locked. Complete the previous chapter first.")]
    ChapterLocked(String),
    #[error("Quiz {quiz_id} max attempts ({max}) reached.")]
    MaxAttemptsReached { quiz_id: String, max: u8 },
    #[error("Course not fully completed. Progress: {0}%")]
    CourseIncomplete(Decimal),
    #[error("Quiz score {score}% is below passing score {required}%")]
    QuizFailed { score: Decimal, required: Decimal },
    #[error("Lesson {0} not found in this chapter")]
    LessonNotFound(String),
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct LmsEngine;

impl LmsEngine {
    /// Records a lesson completion and returns the updated progress percentage.
    pub fn complete_lesson(
        completions: &[LessonCompletion],
        course: &Course,
        student: &str,
        lesson_id: &str,
    ) -> Result<Decimal, LmsError> {
        // Verify lesson exists in the course
        let lesson_exists = course
            .chapters
            .iter()
            .flat_map(|ch| &ch.lessons)
            .any(|l| l.id == lesson_id);

        if !lesson_exists {
            return Err(LmsError::LessonNotFound(lesson_id.to_string()));
        }

        // Count unique completed lessons for this student
        let completed_ids: std::collections::HashSet<&str> = completions
            .iter()
            .filter(|c| c.student == student)
            .map(|c| c.lesson_id.as_str())
            .chain(std::iter::once(lesson_id))
            .collect();

        let total_lessons: u32 = course
            .chapters
            .iter()
            .map(|ch| ch.lessons.len() as u32)
            .sum();

        let completed = completed_ids.len() as u32;
        Ok(calculate_progress(completed, total_lessons))
    }

    /// Grades a quiz attempt and returns the QuizAttempt with score.
    pub fn grade_quiz_attempt(
        quiz: &Quiz,
        answers: Vec<usize>,
        student: &str,
        previous_attempts: u8,
    ) -> Result<QuizAttempt, LmsError> {
        if previous_attempts >= quiz.max_attempts {
            return Err(LmsError::MaxAttemptsReached {
                quiz_id: quiz.id.clone(),
                max: quiz.max_attempts,
            });
        }

        let correct_count = answers
            .iter()
            .zip(quiz.questions.iter())
            .filter(|(answer, q)| **answer == q.correct_option)
            .count();

        let score = if quiz.questions.is_empty() {
            Decimal::ZERO
        } else {
            Decimal::from(correct_count) / Decimal::from(quiz.questions.len()) * dec!(100)
        };

        let passed = score >= quiz.passing_score;

        Ok(QuizAttempt {
            student: student.to_string(),
            quiz_id: quiz.id.clone(),
            answers,
            score,
            passed,
            attempted_at: Utc::now(),
            attempt_number: previous_attempts + 1,
        })
    }

    /// Builds the complete CourseProgress for a student.
    pub fn build_course_progress(
        course: &Course,
        completions: &[LessonCompletion],
        quiz_attempts: &[QuizAttempt],
        student: &str,
    ) -> CourseProgress {
        let completed_lesson_ids: std::collections::HashSet<&str> = completions
            .iter()
            .filter(|c| c.student == student)
            .map(|c| c.lesson_id.as_str())
            .collect();

        let total_lessons: u32 = course
            .chapters
            .iter()
            .map(|ch| ch.lessons.len() as u32)
            .sum();
        let completed_count = completed_lesson_ids.len() as u32;
        let completion_percentage = calculate_progress(completed_count, total_lessons);

        // Build per-chapter statuses and compute unlock chain
        let mut chapter_statuses: Vec<ChapterStatus> = Vec::with_capacity(course.chapters.len());
        let mut previous_chapter_complete = true; // first chapter always unlocked

        for chapter in &course.chapters {
            let ch_lessons_done = chapter
                .lessons
                .iter()
                .filter(|l| completed_lesson_ids.contains(l.id.as_str()))
                .count() as u32;

            let ch_total = chapter.lessons.len() as u32;

            // Quiz passed?
            let quiz_passed = chapter.quiz.as_ref().map_or(true, |q| {
                quiz_attempts
                    .iter()
                    .filter(|a| a.student == student && a.quiz_id == q.id)
                    .any(|a| a.passed)
            });

            let is_complete = ch_lessons_done == ch_total && quiz_passed;
            let is_unlocked = previous_chapter_complete;

            chapter_statuses.push(ChapterStatus {
                chapter_id: chapter.id.clone(),
                chapter_title: chapter.title.clone(),
                lessons_done: ch_lessons_done,
                total_lessons: ch_total,
                quiz_passed,
                is_unlocked,
            });

            previous_chapter_complete = is_complete;
        }

        // Check all quiz scores for certification eligibility
        let all_quiz_scores: Vec<Decimal> = course
            .chapters
            .iter()
            .filter_map(|ch| ch.quiz.as_ref())
            .filter_map(|q| {
                quiz_attempts
                    .iter()
                    .filter(|a| a.student == student && a.quiz_id == q.id && a.passed)
                    .map(|a| a.score)
                    .reduce(Decimal::max)
            })
            .collect();

        let is_certified = verify_certification_eligibility(
            completion_percentage,
            &all_quiz_scores,
            course.passing_score,
        );

        CourseProgress {
            student: student.to_string(),
            course_id: course.id.clone(),
            completion_percentage,
            completed_lessons: completed_count,
            total_lessons,
            chapter_statuses,
            is_certified,
            certificate_id: if is_certified {
                Some(format!(
                    "CERT-{}-{}",
                    course.id.to_uppercase(),
                    student.replace('@', "_")
                ))
            } else {
                None
            },
        }
    }
}

// ── Pure helper functions (also used by erp-learning::learning_progress) ──────

pub fn calculate_progress(completed_lessons: u32, total_lessons: u32) -> Decimal {
    if total_lessons == 0 {
        return Decimal::ZERO;
    }
    Decimal::from(completed_lessons) / Decimal::from(total_lessons) * dec!(100)
}

pub fn verify_certification_eligibility(
    progress: Decimal,
    quiz_scores: &[Decimal],
    passing_score: Decimal,
) -> bool {
    if progress < dec!(100) {
        return false;
    }
    if quiz_scores.is_empty() {
        // Course with no quizzes: completion alone certifies
        return true;
    }
    let total_score: Decimal = quiz_scores.iter().sum();
    let average_score = total_score / Decimal::from(quiz_scores.len());
    average_score >= passing_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_course() -> Course {
        Course {
            id: "rust-fundamentals".to_string(),
            title: "Rust Fundamentals".to_string(),
            description: "Learn Rust from scratch".to_string(),
            instructor: "instructor@example.com".to_string(),
            is_published: true,
            passing_score: dec!(70),
            certificate_template: None,
            chapters: vec![
                Chapter {
                    id: "ch1".to_string(),
                    course_id: "rust-fundamentals".to_string(),
                    title: "Ownership".to_string(),
                    order_index: 1,
                    lessons: vec![
                        Lesson { id: "l1".to_string(), chapter_id: "ch1".to_string(), title: "Ownership basics".to_string(), content_type: ContentType::Article, content_url: None, content_body: Some("...".to_string()), order_index: 1, duration_minutes: 15 },
                        Lesson { id: "l2".to_string(), chapter_id: "ch1".to_string(), title: "Borrowing".to_string(), content_type: ContentType::Video, content_url: Some("https://example.com/v/borrow".to_string()), content_body: None, order_index: 2, duration_minutes: 20 },
                    ],
                    quiz: Some(Quiz {
                        id: "q1".to_string(),
                        chapter_id: "ch1".to_string(),
                        title: "Ownership Quiz".to_string(),
                        passing_score: dec!(70),
                        max_attempts: 3,
                        questions: vec![
                            QuizQuestion {
                                id: "qq1".to_string(),
                                question_text: "What does 'move' semantics mean?".to_string(),
                                options: vec!["Copy the value".to_string(), "Transfer ownership".to_string(), "Delete the value".to_string()],
                                correct_option: 1,
                                explanation: Some("Move transfers ownership to the new binding.".to_string()),
                            },
                            QuizQuestion {
                                id: "qq2".to_string(),
                                question_text: "How many immutable borrows are allowed at once?".to_string(),
                                options: vec!["1".to_string(), "0".to_string(), "Unlimited".to_string()],
                                correct_option: 2,
                                explanation: Some("Multiple immutable borrows are allowed simultaneously.".to_string()),
                            },
                        ],
                    }),
                },
            ],
        }
    }

    #[test]
    fn progress_calculation_partial() {
        let p = calculate_progress(1, 2);
        assert_eq!(p, dec!(50));
    }

    #[test]
    fn progress_zero_lessons_is_zero() {
        assert_eq!(calculate_progress(0, 0), Decimal::ZERO);
    }

    #[test]
    fn quiz_grading_perfect_score() {
        let course = sample_course();
        let quiz = course.chapters[0].quiz.as_ref().unwrap();
        let attempt = LmsEngine::grade_quiz_attempt(
            quiz,
            vec![1, 2], // both correct
            "student@test.com",
            0,
        )
        .unwrap();
        assert_eq!(attempt.score, dec!(100));
        assert!(attempt.passed);
    }

    #[test]
    fn quiz_grading_fail() {
        let course = sample_course();
        let quiz = course.chapters[0].quiz.as_ref().unwrap();
        let attempt = LmsEngine::grade_quiz_attempt(
            quiz,
            vec![0, 0], // both wrong
            "student@test.com",
            0,
        )
        .unwrap();
        assert_eq!(attempt.score, Decimal::ZERO);
        assert!(!attempt.passed);
    }

    #[test]
    fn max_attempts_exceeded() {
        let course = sample_course();
        let quiz = course.chapters[0].quiz.as_ref().unwrap();
        let result = LmsEngine::grade_quiz_attempt(quiz, vec![], "s@t.com", 3);
        assert!(matches!(result, Err(LmsError::MaxAttemptsReached { .. })));
    }

    #[test]
    fn certification_eligibility_full_completion() {
        let eligible = verify_certification_eligibility(dec!(100), &[dec!(85), dec!(90)], dec!(70));
        assert!(eligible, "Should be eligible with 100% progress and avg score above passing");
    }

    #[test]
    fn certification_not_eligible_incomplete() {
        let not_eligible = verify_certification_eligibility(dec!(80), &[dec!(90)], dec!(70));
        assert!(!not_eligible, "Cannot certify without 100% lesson completion");
    }

    #[test]
    fn course_progress_chapter_unlock_chain() {
        let course = sample_course();
        let completions = vec![
            LessonCompletion { student: "s@t.com".to_string(), lesson_id: "l1".to_string(), completed_at: Utc::now(), time_spent_minutes: 15 },
            LessonCompletion { student: "s@t.com".to_string(), lesson_id: "l2".to_string(), completed_at: Utc::now(), time_spent_minutes: 20 },
        ];
        let quiz_attempt = QuizAttempt {
            student: "s@t.com".to_string(),
            quiz_id: "q1".to_string(),
            answers: vec![1, 2],
            score: dec!(100),
            passed: true,
            attempted_at: Utc::now(),
            attempt_number: 1,
        };
        let progress = LmsEngine::build_course_progress(
            &course,
            &completions,
            &[quiz_attempt],
            "s@t.com",
        );
        assert_eq!(progress.completion_percentage, dec!(100));
        assert!(progress.is_certified);
        assert!(progress.certificate_id.is_some());
    }
}
