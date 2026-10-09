import type { ReactNode } from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import styles from './index.module.css';

export default function Home(): ReactNode {
  return (
    <Layout
      title="Your Insomnia collections, in a native Rust app"
      description="Venus is a native Rust API client for Insomnia users, with browser sign-in, request and environment sync, and commit previews.">
      <section className={styles.hero}>
        <div className={styles.heroContainer}>
          <h1 className={styles.heroTitle}>Venus</h1>
          <p className={styles.heroSub}>
            Work with your Insomnia collections without running Insomnia.
            A native Rust API client with browser sign-in, request and environment
            sync, and a review before every cloud push.
          </p>
          <div className={styles.heroCta}>
            <Link className={styles.btnPrimary} to="/docs/getting-started">
              Build and install
            </Link>
            <Link className={styles.btnSecondary} to="/docs/features/import-export">
              Connect your collections
            </Link>
          </div>
        </div>
      </section>
      <section className={styles.manifesto}>
        <div className={styles.manifestoContainer}>
          <h2 className={styles.manifestoTitle}>Keep your team's workflow.</h2>
          <p className={styles.manifestoText}>
            Sign in to your existing Insomnia account through your browser.
            Choose a collection and download master into a local folder.
            No export is needed, and your teammates can continue using Insomnia.
          </p>
          <p className={styles.manifestoText}>
            Requests stay in editable JSON files. Cloud environments preserve
            native values and base inheritance. Review changes, write a commit
            message, and push to share them with your team.
          </p>
          <p className={styles.manifestoText}>
            Cloud sync runs when you choose it. Ordinary dotenv files stay local.
            Venus is an independent fork of{' '}
            <a href="https://github.com/Harry-kp/mercury">Mercury</a> by
            Harshit Chaudhary, retaining its MIT license.
          </p>
          <Link to="https://github.com/alvezgr/venus">View the Venus source</Link>
        </div>
      </section>
    </Layout>
  );
}
